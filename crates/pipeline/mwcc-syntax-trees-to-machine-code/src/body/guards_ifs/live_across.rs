//! Values live across a branch: float-param reassign and general live-across-branches shapes.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    /// FLOAT PARAM REASSIGNMENT: `if (c) { x = -x; } return <expr of x>;` —
    /// the live float stays IN ITS PARAM REGISTER (an in-place fneg; measured,
    /// and `double t = x; if (c) t = -x;` canonicalizes identically). The
    /// bare-copy local aliases to the param when the param is otherwise dead.
    pub(crate) fn try_float_param_reassign(&mut self, function: &Function) -> Compilation<bool> {
        // The only "calls" allowed are the __fabs INTRINSIC in the arms
        // (a single fabs instruction, not a real call — checked per arm below).
        let has_real_call = function.return_expression.as_ref().is_some_and(crate::analysis::expression_has_call)
            || function.locals.iter().any(|local| local.initializer.as_ref().is_some_and(crate::analysis::expression_has_call));
        if !matches!(function.return_type, Type::Float | Type::Double)
            || function.return_expression.is_none()
            || !function.guards.is_empty()
            || has_real_call
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // An optional single bare-copy local (`double t = x;`) aliases to the
        // param; more locals are outside this slice.
        let mut alias: Option<(&str, &str)> = None;
        match function.locals.as_slice() {
            [] => {}
            [local]
                if matches!(local.declared_type, Type::Float | Type::Double)
                    && !local.is_static
                    && local.array_length.is_none() =>
            {
                let Some(Expression::Variable(source)) = &local.initializer else { return Ok(false) };
                if self.float_register_of(source).is_err() {
                    return Ok(false);
                }
                alias = Some((local.name.as_str(), source.as_str()));
            }
            _ => return Ok(false),
        }
        fn resolve<'a>(alias: Option<(&'a str, &'a str)>, name: &'a str) -> &'a str {
            match alias {
                Some((local, source)) if local == name => source,
                _ => name,
            }
        }
        // Statements: `if (int-param cmp const) { fparam = -fparam; }` runs.
        let mut reassigns: Vec<(&Expression, &str, bool)> = Vec::new();
        for statement in &function.statements {
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() || then_body.len() != 1 {
                return Ok(false);
            }
            let condition_ok = match condition {
                Expression::Variable(name) => self.lookup_general(name).is_some(),
                Expression::Binary { left, right, .. } => {
                    matches!(left.as_ref(), Expression::Variable(name) if self.lookup_general(name).is_some())
                        && constant_value(right).is_some()
                }
                _ => false,
            };
            if !condition_ok {
                return Ok(false);
            }
            let Statement::Assign { name, value } = &then_body[0] else { return Ok(false) };
            let target = resolve(alias, name);
            // `x = -x` (fneg) or `x = __fabs(x)` (the fabs instruction).
            let (source, is_abs) = match value {
                Expression::Unary { operator: UnaryOperator::Negate, operand } => match operand.as_ref() {
                    Expression::Variable(source) => (source, false),
                    _ => return Ok(false),
                },
                Expression::Call { name: callee, arguments } if is_intrinsic_call(callee) => match arguments.as_slice() {
                    [Expression::Variable(source)] => (source, true),
                    _ => return Ok(false),
                },
                _ => return Ok(false),
            };
            if resolve(alias, source) != target || self.float_register_of(target).is_err() {
                return Ok(false);
            }
            reassigns.push((condition, target, is_abs));
        }
        if reassigns.is_empty() {
            return Ok(false);
        }
        // The aliased param must not be read under its own name afterwards
        // (the alias takes the register over).
        let return_expression = function.return_expression.as_ref().expect("gated");
        if let Some((local, source)) = alias {
            if count_name_occurrences(return_expression, source) > 0 {
                return Ok(false);
            }
            let register = self.float_register_of(source).expect("checked");
            self.locations.insert(local.to_string(), crate::generator::Location {
                class: crate::generator::ValueClass::Float,
                register,
                signed: true,
                width: if function.return_type == Type::Float { 32 } else { 64 },
                pointee: None,
                stride: None,
            });
        }
        // Each if's join label advances mwcc's anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * reassigns.len() as u32;
        for (condition, target, is_abs) in &reassigns {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            let register = self.float_register_of(target).expect("checked");
            self.output.instructions.push(if *is_abs {
                Instruction::FloatAbsolute { d: register, b: register }
            } else {
                Instruction::FloatNegate { d: register, b: register }
            });
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        let result = Eabi::float_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// LIVE-ACROSS-BRANCHES: initialized int locals reassigned inside simple
    /// if-blocks, read after the joins (the s_atan `id`/`x` skeleton). The
    /// measured model: the condition's cmpwi leads; the inits compute
    /// SPECULATIVELY before the branch; every definition of one local shares
    /// ONE register home — r0 first unless a later use forbids it (an addi
    /// source), else the condition's DYING register, else a free volatile —
    /// and the trailing return/guards consume the locals as pseudo-params.
    pub(crate) fn try_live_across_branches(&mut self, function: &Function) -> Compilation<bool> {
        let int_return = function.return_type == Type::Int && function.return_expression.is_some();
        let void_stores = function.return_type == Type::Void && function.return_expression.is_none();
        if !(int_return || void_stores)
            || function_makes_call(function)
            || function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        if void_stores && !function.guards.is_empty() {
            return Ok(false);
        }
        // Trailing guards (`if (id < 0) return a;` — the id-tested-later form)
        // are allowed: their conditions/values may read the live locals, which
        // resolve through the registered home locations below.
        for guard in &function.guards {
            if !matches!(&guard.condition, Expression::Variable(_) | Expression::Binary { .. }) {
                return Ok(false);
            }
        }
        // Every local: int, initialized, non-static.
        if function.locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || local.initializer.is_none()
                || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        }) {
            return Ok(false);
        }
        // The statements: a run of `if (param <cmp> const) { local = value; ... }`
        // blocks (no else), reassigning ONLY the declared locals.
        let local_names: Vec<&str> = function.locals.iter().map(|local| local.name.as_str()).collect();
        let simple_value = |expression: &Expression| -> bool {
            let readable = |name: &str| self.lookup_general(name).is_some() || local_names.contains(&name);
            match expression {
                Expression::IntegerLiteral(value) => i16::try_from(*value).is_ok(),
                Expression::Variable(name) => readable(name.as_str()),
                Expression::Binary { operator, left, right } => {
                    matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply)
                        && matches!(left.as_ref(), Expression::Variable(name) if readable(name.as_str()))
                        && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok())
                }
                _ => false,
            }
        };
        // A VOID body: a run of ifs then TRAILING STORES to distinct SDA int
        // globals (the tail — DAG-scheduled below with the live locals as
        // pseudo-params).
        let mut tail_stores: Vec<&Statement> = Vec::new();
        let mut branch_conditions: Vec<&Expression> = Vec::new();
        for statement in &function.statements {
            if let Statement::Store { target, value } = statement {
                if !void_stores {
                    return Ok(false);
                }
                let Expression::Variable(global) = target else { return Ok(false) };
                if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                    return Ok(false);
                }
                if !simple_value(value) {
                    return Ok(false);
                }
                tail_stores.push(statement);
                continue;
            }
            if !tail_stores.is_empty() {
                // A branch after the tail began — outside this slice.
                return Ok(false);
            }
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() {
                return Ok(false);
            }
            // The condition: a bare param, or param <cmp> constant.
            let condition_param = match condition {
                Expression::Variable(name) => Some(name.as_str()),
                Expression::Binary { left, right, .. } => match (left.as_ref(), constant_value(right)) {
                    (Expression::Variable(name), Some(_)) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            };
            let Some(condition_param) = condition_param else { return Ok(false) };
            if self.lookup_general(condition_param).is_none() || local_names.contains(&condition_param) {
                return Ok(false);
            }
            // A NARROW condition operand (`unsigned char t`) — mwcc interleaves the
            // speculative inits into the width-op -> compare latency gap (`clrlwi;
            // li; cmplwi; li; …`) and may reuse the test scratch r0 as a local's home
            // once the compare consumes it — a schedule/allocation this handler's
            // "cmpwi leads, inits follow" model does not reproduce (measured DIFF,
            // fire 644). Defer.
            if self
                .locations
                .get(condition_param)
                .is_some_and(|location| location.width < 32)
            {
                return Ok(false);
            }
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { return Ok(false) };
                if !local_names.contains(&name.as_str()) || !simple_value(value) {
                    return Ok(false);
                }
            }
            branch_conditions.push(condition);
        }
        if branch_conditions.is_empty() || (void_stores && tail_stores.is_empty()) {
            return Ok(false);
        }
        // Every declared local must be REASSIGNED in some branch. An UNMUTATED
        // const-init local is FOLDED by mwcc into its consumer instead of living in
        // a register (`int b=4; … return a+b+c` -> `add a,c; addi r3,r3,4` — measured
        // DIFF, fire 644), a fold this handler does not model. Defer.
        let assigned: Vec<&str> = function
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::If { then_body, .. } => Some(then_body),
                _ => None,
            })
            .flatten()
            .filter_map(|inner| match inner {
                Statement::Assign { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        if function.locals.iter().any(|local| !assigned.contains(&local.name.as_str())) {
            return Ok(false);
        }
        // Init values must be simple too.
        for local in &function.locals {
            if !simple_value(local.initializer.as_ref().expect("gated")) {
                return Ok(false);
            }
        }
        // HOME SELECTION. A use as an addi source forbids r0: an init/arm value
        // `local <op> const` reading the local, the return expression adding a
        // constant to it, or a tail store's value doing the same.
        let forbids_r0 = |name: &str| -> bool {
            let reads_as_addi = |expression: &Expression| -> bool {
                matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(inner) if inner == name) && constant_value(right).is_some())
            };
            if function.return_expression.as_ref().is_some_and(&reads_as_addi) {
                return true;
            }
            if tail_stores.iter().any(|statement| matches!(statement, Statement::Store { value, .. } if reads_as_addi(value))) {
                return true;
            }
            function.statements.iter().any(|statement| {
                let Statement::If { then_body, .. } = statement else { return false };
                then_body.iter().any(|inner| matches!(inner, Statement::Assign { value, .. } if reads_as_addi(value)))
            })
        };
        // Dying condition registers: a condition param never referenced later.
        let mut dying_condition_registers: Vec<u8> = Vec::new();
        for condition in &branch_conditions {
            let param = match condition {
                Expression::Variable(name) => name.as_str(),
                Expression::Binary { left, .. } => match left.as_ref() {
                    Expression::Variable(name) => name.as_str(),
                    _ => continue,
                },
                _ => continue,
            };
            let uses_elsewhere = function.return_expression.as_ref().map_or(0, |expression| count_name_occurrences(expression, param))
                + function
                    .statements
                    .iter()
                    .map(|statement| statement_reads(statement, param))
                    .sum::<usize>()
                > branch_conditions
                    .iter()
                    .filter(|other| {
                        matches!(other, Expression::Variable(name) if name == param)
                            || matches!(other, Expression::Binary { left, .. } if matches!(left.as_ref(), Expression::Variable(name) if name == param))
                    })
                    .count();
            if !uses_elsewhere {
                if let Some(register) = self.lookup_general(param) {
                    dying_condition_registers.push(register);
                }
            }
        }
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut taken: Vec<u8> = Vec::new();
        for local in &function.locals {
            // In a VOID body, r0 belongs to the LAST tail chain: the local may
            // take it only when it IS that chain's value (stored bare by the
            // final store, read nowhere else in the tail).
            let r0_ok = if void_stores {
                let last_is_bare_self = matches!(
                    tail_stores.last(),
                    Some(Statement::Store { value: Expression::Variable(name), .. }) if *name == local.name
                );
                let tail_reads: usize = tail_stores
                    .iter()
                    .map(|statement| statement_reads(statement, &local.name))
                    .sum();
                last_is_bare_self && tail_reads == 1 && !forbids_r0(&local.name)
            } else {
                !forbids_r0(&local.name)
            };
            let candidates: Vec<u8> = if !r0_ok {
                dying_condition_registers.iter().copied().chain(5..=12).collect()
            } else {
                std::iter::once(0u8).chain(dying_condition_registers.iter().copied()).chain(5..=12).collect()
            };
            let Some(register) = candidates.into_iter().find(|register| !taken.contains(register)) else {
                return Ok(false);
            };
            taken.push(register);
            homes.push((local.name.clone(), register));
        }
        let home_of = |name: &str| homes.iter().find(|(local, _)| local == name).map(|&(_, register)| register);

        // EMISSION. First branch: cmpwi, speculative inits, branch; later
        // branches: cmpwi, branch, arm. Each if's join label advances mwcc's
        // anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * branch_conditions.len() as u32;
        for (index, statement) in function.statements.iter().enumerate() {
            // Tail stores emit after the branch structure.
            let Statement::If { condition, then_body, .. } = statement else { break };
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if index == 0 {
                for local in &function.locals {
                    let home = home_of(&local.name).expect("assigned");
                    self.evaluate(local.initializer.as_ref().expect("gated"), Type::Int, home)?;
                }
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { unreachable!() };
                // A reassignment may read the local itself (its home).
                let home = home_of(name).expect("assigned");
                self.evaluate_with_live_locals(value, home, &homes)?;
            }
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        // The trailing return consumes the locals as pseudo-params.
        for (name, register) in &homes {
            self.locations.insert(name.clone(), crate::generator::Location {
                class: crate::generator::ValueClass::General,
                register: *register,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            });
        }
        if void_stores {
            // The tail: a single bare-local store emits directly; a richer run
            // routes through the DAG store-fill with the live locals as
            // PSEUDO-PARAMS (their homes registered above resolve through
            // lookup_general like any parameter).
            if let [Statement::Store { target: Expression::Variable(global), value: Expression::Variable(name) }] =
                tail_stores.as_slice()
            {
                let source = self.lookup_general(name).expect("registered home");
                self.record_relocation(RelocationKind::EmbSda21, global);
                self.output.instructions.push(Instruction::StoreWord { s: source, a: 0, offset: 0 });
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            let mut pseudo = function.parameters.clone();
            for (name, _) in &homes {
                pseudo.push(mwcc_syntax_trees::Parameter { parameter_type: Type::Int, name: name.clone() });
            }
            let synthesized = Function {
                return_type: Type::Void,
                section: None,
                asm_body: None, force_active: false,
                name: function.name.clone(),
                is_static: function.is_static,
                is_weak: function.is_weak,
                text_deferred: false,
                parameters: pseudo,
                locals: Vec::new(),
                statements: tail_stores.iter().map(|&statement| statement.clone()).collect(),
                guards: Vec::new(),
                return_expression: None,
            };
            if !self.try_dag_store_fill(&synthesized)? {
                return Err(Diagnostic::error("a live-across store tail outside the DAG envelope needs more vocabulary (roadmap)"));
            }
            return Ok(true);
        }
        let return_expression = function.return_expression.as_ref().expect("gated");
        let result = Eabi::general_result().number;
        if function.guards.is_empty() {
            self.evaluate_tail(return_expression, Type::Int, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        } else {
            self.emit_guard_sequence(&function.guards, return_expression, Type::Int, result)?;
        }
        Ok(true)
    }

}
