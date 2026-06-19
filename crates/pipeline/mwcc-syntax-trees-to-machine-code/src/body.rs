//! Function-level emission: parameters, body, guards, and the return tail.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    pub(crate) fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            let signed = self.signed_of(parameter.parameter_type);
            let pointee = match parameter.parameter_type {
                Type::Pointer(pointee) => Some(pointee),
                _ => None,
            };
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed, width: parameter.parameter_type.width(), pointee },
            );
        }
        Ok(())
    }

    /// Emit the whole function body, including its `blr`(s).
    pub(crate) fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        // A function that takes the address of a variable lowers it to a stack
        // slot (frame-resident); this takes over the whole body. Checked first,
        // since an address-taken variable cannot be value-tracked in a register.
        if self.try_frame_resident(function)? {
            return Ok(());
        }
        // Value-tracked locals (reassignment, multiple locals) are inlined into the
        // return expression and compiled there; this takes over the whole body when
        // it applies, leaving the straight-line paths below byte-identical.
        if self.try_value_tracking(function)? {
            return Ok(());
        }
        // A leaf void body that is purely constant stores of one repeated value
        // (struct/array zeroing) materializes the value once and reuses it.
        if self.try_constant_store_fill(function)? {
            return Ok(());
        }
        // A function whose body is a single `switch` lowers to the dispatch tree:
        // the comparisons, then the case bodies, then the default (the `default:`
        // arm if present, else the function's trailing `return`). The cases and
        // default each end in their own `blr`, so this owns the whole body.
        if let [Statement::Switch { scrutinee, arms, default }] = function.statements.as_slice() {
            if function.guards.is_empty() && function.locals.is_empty() && !function_makes_call(function) {
                let default_expression = default
                    .as_ref()
                    .or(function.return_expression.as_ref())
                    .ok_or_else(|| Diagnostic::error("a switch with no default needs a trailing return"))?;
                let result = match function.return_type {
                    Type::Float | Type::Double => return Err(Diagnostic::error("a floating-point switch result is not supported yet (roadmap)")),
                    Type::Void => return Err(Diagnostic::error("a void switch is not supported yet (roadmap)")),
                    _ => Eabi::general_result().number,
                };
                return self.emit_switch(scrutinee, arms, default_expression, default.is_some(), function.return_type, result);
            }
        }
        // A non-leaf function whose whole body is `if (c) <call>;`: mwcc schedules
        // the condition test (`cmpwi`) into the prologue, between `mflr` and the LR
        // store, then branches forward over the body to the epilogue when false.
        if let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() {
            if function_makes_call(function)
                && function.return_type == Type::Void
                && function.guards.is_empty()
                && else_body.is_empty()
                && then_body.len() == 1
            {
                self.non_leaf = true;
                self.frame_size = 16;
                self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
                self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = label;
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // A function that calls is non-leaf: save the link register around a 16-byte
        // frame before doing anything else.
        if function_makes_call(function) {
            if !function.guards.is_empty() {
                return Err(Diagnostic::error("calls combined with guards not yet supported"));
            }
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        }

        // Body statements (stores, calls) run first.
        let statement_count = function.statements.len();
        for (index, statement) in function.statements.iter().enumerate() {
            // A trailing `if (c) { body }` in a leaf void function: the false path
            // is the function exit, so it is a conditional return, then the body,
            // then the normal `blr`. (Non-leaf needs a forward branch to the
            // epilogue, and a non-final if needs to skip forward — both deferred.)
            if let Statement::If { condition, then_body, else_body } = statement {
                // Single-statement leaf if-blocks. A multi-statement body needs the
                // instruction scheduler, and a non-leaf if needs the cmpwi scheduled
                // into the prologue — both defer for now.
                if then_body.len() == 1 && !function_makes_call(function) {
                    let trailing_void = index + 1 == statement_count && function.return_type == Type::Void;
                    if trailing_void {
                        // The false path is the function exit (or the else / else-if):
                        // a conditional return, or a branch into the else chain.
                        self.emit_trailing_if(condition, then_body, else_body)?;
                        continue;
                    }
                    if else_body.is_empty() {
                        // The false path skips the body: forward branch.
                        self.emit_if_forward(condition, then_body)?;
                        continue;
                    }
                }
            }
            self.emit_statement(statement)?;
        }

        // A `void` function ends after its statements.
        if function.return_type == Type::Void {
            self.emit_epilogue_and_return();
            return Ok(());
        }

        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;

        if !function.guards.is_empty() {
            if !function.locals.is_empty() {
                return Err(Diagnostic::error("locals combined with guards not yet supported"));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                let select = Expression::Conditional {
                    condition: Box::new(guard.condition.clone()),
                    when_true: Box::new(guard.value.clone()),
                    when_false: Box::new(return_expression.clone()),
                };
                self.evaluate_tail(&select, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }
            return self.emit_guard_sequence(&function.guards, return_expression, function.return_type, result);
        }

        match function.locals.as_slice() {
            [] => self.evaluate_tail(return_expression, function.return_type, result)?,
            [local] => self.evaluate_single_local(local, return_expression, function.return_type, result)?,
            _ => return Err(Diagnostic::error("multiple locals need the full register allocator (roadmap M1)")),
        }
        // A `float` function returning a double-precision value rounds to single
        // (`frsp`) before returning, as mwcc does.
        if function.return_type == Type::Float && self.is_double_value(return_expression) {
            self.output.instructions.push(Instruction::RoundToSingle { d: result, b: result });
        }
        self.emit_epilogue_and_return();
        Ok(())
    }

    /// A leaf `void` body that is purely constant stores: mwcc materializes a
    /// repeated store value once and reuses the register (`li r0,0; stw; stw; stw`
    /// for struct/array zeroing). A run of *differing* constants instead needs the
    /// instruction scheduler (distinct registers, interleaved) — defer rather than
    /// emit the unscheduled form. Returns `false` (use the normal path) for bodies
    /// outside this shape, e.g. stores of register-resident values, which already
    /// match.
    pub(crate) fn try_constant_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() < 2
        {
            return Ok(false);
        }
        let mut constants = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            if !self.is_scratch_safe_store_target(target) {
                return Ok(false);
            }
            match constant_value(value) {
                Some(constant) => constants.push(constant as i32),
                None => return Ok(false),
            }
        }
        if constants.iter().any(|constant| *constant != constants[0]) {
            // Two distinct constants: mwcc materializes both up front (the first
            // into a free register, the second into the scratch), then stores both
            // — `li r4,v1; li r0,v2; stw r4; stw r0`. Three or more interleave on a
            // sliding window the scheduler models; defer those.
            if constants.len() != 2 {
                return Err(Diagnostic::error("a run of 3+ differing constant stores needs the scheduler (roadmap)"));
            }
            let base_registers: Vec<u8> = function.statements.iter()
                .filter_map(|statement| match statement {
                    Statement::Store { target, .. } => self.store_base_register(target),
                    _ => None,
                })
                .collect();
            let Some(first_register) = (3u8..=12).find(|r| *r != GENERAL_SCRATCH && !base_registers.contains(r) && !self.reserved.contains(r)) else {
                return Err(Diagnostic::error("no free register for the two-constant store run"));
            };
            self.load_integer_constant(first_register, constants[0] as i64);
            self.load_integer_constant(GENERAL_SCRATCH, constants[1] as i64);
            self.prematerialized_constants = vec![(constants[0], first_register), (constants[1], GENERAL_SCRATCH)];
            for statement in &function.statements {
                self.emit_statement(statement)?;
            }
            self.prematerialized_constants.clear();
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        self.reuse_scratch_constant = true;
        self.scratch_constant = None;
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        self.reuse_scratch_constant = false;
        self.scratch_constant = None;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Whether a store to `target` writes only memory (and the value register),
    /// never the scratch — so a constant-fill run can keep its value live in the
    /// scratch across it. A leaf-based member/dereference/constant-index store
    /// qualifies; a global (absolute-addressing base) or variable index (scratch
    /// scaling) does not.
    fn is_scratch_safe_store_target(&self, target: &Expression) -> bool {
        match target {
            Expression::Member { base, .. } => matches!(base.as_ref(), Expression::Variable(_)),
            Expression::Dereference { pointer } => matches!(pointer.as_ref(), Expression::Variable(_)),
            Expression::Index { base, index } => {
                matches!(base.as_ref(), Expression::Variable(_)) && constant_value(index).is_some()
            }
            _ => false,
        }
    }

    /// The register holding the base pointer of a scratch-safe store target.
    fn store_base_register(&self, target: &Expression) -> Option<u8> {
        let name = match target {
            Expression::Member { base, .. } | Expression::Index { base, .. } => leaf_name(base),
            Expression::Dereference { pointer } => leaf_name(pointer),
            _ => None,
        }?;
        self.lookup_general(name)
    }

    /// Tear down the stack frame (if one was allocated) and return. A non-leaf
    /// function restores the link register from `frame_size + 4` first.
    pub(crate) fn emit_epilogue_and_return(&mut self) {
        if self.non_leaf {
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        }
        if self.frame_size != 0 {
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
    }

    /// Emit a body statement.
    pub(crate) fn emit_statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Store { target, value } => self.emit_store(target, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.emit_call(name, arguments, None, false)
            }
            Statement::Expression(_) => Err(Diagnostic::error("only a call may be a bare statement (roadmap)")),
            // Reassignment is handled by value tracking; reaching here means it was
            // mixed with stores/calls, which that path defers.
            Statement::Assign { .. } => Err(Diagnostic::error("local reassignment mixed with stores/calls is not supported yet (roadmap)")),
            // The binary-search dispatch codegen is the next piece; switches parse
            // but defer for now (never miscompile).
            Statement::Switch { .. } => Err(Diagnostic::error("switch dispatch codegen is not implemented yet (roadmap)")),
            // A general if-statement (non-trailing, non-leaf, or with an else) needs
            // forward branches and basic-block scheduling — deferred for now.
            Statement::If { .. } => Err(Diagnostic::error("general if-statement codegen is not implemented yet (roadmap)")),
            // An early `return` inside the body needs early-return codegen (blr for
            // a leaf, a forward branch to the shared epilogue otherwise) — the
            // parser now models it, but the codegen is the next piece.
            Statement::Return(_) => Err(Diagnostic::error("early-return codegen is not implemented yet (roadmap)")),
        }
    }

    /// A trailing leaf `if (c) then; [else otherwise | else if …]` in a void
    /// function. With no else, the false path is a conditional return (the body
    /// then falls through to the function `blr`). With an else, branch over the
    /// then-body (and its `blr`) to the else, which is either a single statement
    /// or a nested trailing if (an `else if` chain). Each then-body is a single
    /// statement — multiple statements need the scheduler.
    fn emit_trailing_if(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement]) -> Compilation<()> {
        if then_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement if-body needs the scheduler (roadmap)"));
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if else_body.is_empty() {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            return self.emit_statement(&then_body[0]);
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.emit_statement(&then_body[0])?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        // The else: a nested trailing `if` (an `else if`), or a single statement.
        if let [Statement::If { condition, then_body, else_body }] = else_body {
            self.emit_trailing_if(condition, then_body, else_body)?;
        } else if else_body.len() == 1 {
            self.emit_statement(&else_body[0])?;
        } else {
            return Err(Diagnostic::error("a multi-statement else-body needs the scheduler (roadmap)"));
        }
        Ok(())
    }

    /// A non-trailing `if (c) { body }`: the false path branches forward over the
    /// body to the code that follows.
    fn emit_if_forward(&mut self, condition: &Expression, then_body: &[Statement]) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        Ok(())
    }

    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    pub(crate) fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => self.locations.get(name).map(|location| location.register) == Some(result),
            _ => false,
        };

        for (index, guard) in guards.iter().enumerate() {
            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            let value_register = self.general_register_of_leaf(&guard.value)?;
            let is_last = index + 1 == guards.len();

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            if result != value_register {
                self.output.instructions.push(Instruction::move_register(result, value_register));
            }
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

    /// Evaluate the function result. A conditional in this tail position can use a
    /// conditional return when one of its values already sits in the result register.
    pub(crate) fn evaluate_tail(&mut self, expression: &Expression, value_type: Type, result: u8) -> Compilation<()> {
        match expression {
            Expression::Conditional { condition, when_true, when_false } => match value_type {
                Type::Float | Type::Double => self.emit_float_conditional(condition, when_true, when_false, result, true),
                _ => self.emit_conditional(condition, when_true, when_false, result, true),
            },
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                self.emit_short_circuit(*operator, left, right, result)
            }
            // A narrow return type truncates the returned value. A `(type)` cast
            // expression already yields the narrow type, so it falls through to the
            // normal path; everything else is coerced here.
            other if is_narrow_int(value_type) && !matches!(other, Expression::Cast { .. }) => {
                self.evaluate_narrow_return(other, value_type, result)
            }
            other => self.evaluate(other, value_type, result),
        }
    }

    pub(crate) fn evaluate_single_local(
        &mut self,
        local: &LocalDeclaration,
        return_expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let class = class_of(local.declared_type)?;
        // The single-local straight-line path needs the local's initializer; an
        // uninitialized local (its value comes from an assignment) is value-tracked.
        let initializer = local
            .initializer
            .as_ref()
            .ok_or_else(|| Diagnostic::error("an uninitialized single local is not supported here (roadmap)"))?;

        // `return x;` — the local is the result, so compute its initializer
        // straight into the result register.
        if matches!(return_expression, Expression::Variable(name) if *name == local.name) {
            return self.evaluate(initializer, local.declared_type, result);
        }

        // Otherwise the local lives in the scratch register and is used as a leaf.
        // That only works if the result expression does not itself need the scratch.
        if needs_scratch(return_expression) {
            return Err(Diagnostic::error("local reused inside a scratch-needing expression (roadmap M1)"));
        }
        let scratch = match class {
            ValueClass::General => GENERAL_SCRATCH,
            ValueClass::Float => FLOAT_SCRATCH,
        };
        self.evaluate(initializer, local.declared_type, scratch)?;
        let signed = self.signed_of(local.declared_type);
        let pointee = match local.declared_type {
            Type::Pointer(pointee) => Some(pointee),
            _ => None,
        };
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed, width: local.declared_type.width(), pointee });
        self.evaluate(return_expression, return_type, result)
    }

    pub(crate) fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        match value_type {
            // A `double` shares the FPR file with `float`; the float path picks the
            // double-precision instructions via is_double_value. An integer leaf in
            // a float context is an implicit int->float conversion (the same magic-
            // constant sequence as the explicit `(float)`/`(double)` cast).
            Type::Float | Type::Double => {
                if self.is_integer_leaf(expression) {
                    return self.emit_cast_to_float(expression, destination, value_type == Type::Double);
                }
                self.evaluate_float(expression, destination)
            }
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
            // A float leaf in an integer context is an implicit float->int conversion
            // (the same `fctiwz` + frame bounce as the explicit `(int)` cast).
            _ => {
                if self.is_float_value(expression) {
                    return self.emit_cast_to_integer(value_type, expression, destination);
                }
                // A whole signed-`char` load promoted to `int` sign-extends the
                // loaded byte: `lbz d,…; extsb d,d`. (`lbz` zero-extends, so the
                // promotion needs the trailing `extsb`; the narrow-return path
                // calls `evaluate_general` directly and so keeps the bare `lbz`.)
                if matches!(value_type, Type::Int | Type::UnsignedInt) && self.is_signed_byte_load(expression)? {
                    self.evaluate_general(expression, destination)?;
                    self.emit_widen(destination, destination, 8, true);
                    return Ok(());
                }
                self.evaluate_general(expression, destination)
            }
        }
    }

    /// Whether `expression` is a full-width integer leaf variable (an int/unsigned
    /// in a GPR, not a pointer or a narrow type) — the operand an implicit
    /// int->float conversion accepts.
    fn is_integer_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str())
                .is_some_and(|location| location.class == ValueClass::General && location.width == 32 && location.pointee.is_none()))
    }
}
