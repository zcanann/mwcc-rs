//! Function-level emission: parameters, body, guards, and the return tail.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, LoopKind, Statement, Type, UnaryOperator};
use mwcc_versions::GlobalAddressing;
use crate::expressions::pointee_of_type;

/// The `(operand, constant)` a guard condition compares against, when it is `<var> OP <const>`
/// (or the commuted `<const> OP <var>`). Two consecutive guards with the same key share one
/// `cmpwi` in mwcc, which emit_guard_sequence does not model (so it defers such a pair).
fn guard_comparison_key(condition: &Expression) -> Option<(String, i64)> {
    let Expression::Binary { operator, left, right } = condition else { return None };
    if !matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
    ) {
        return None;
    }
    if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), constant_value(right)) {
        return Some((name.clone(), constant));
    }
    if let (Some(constant), Expression::Variable(name)) = (constant_value(left), right.as_ref()) {
        return Some((name.clone(), constant));
    }
    None
}

/// A safe-when-nonzero access of a register pointer `p` — `*p`, `p[const]`, or `p->field` — the kind
/// of dereference a null guard protects. (A variable index `p[i]` needs its scaled register live, so
/// it is excluded.)
fn accesses_pointer(expression: &Expression, pointer: &str) -> bool {
    let is_pointer = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == pointer);
    match expression {
        Expression::Dereference { pointer: inner } => is_pointer(inner.as_ref()),
        Expression::Index { base, index } => is_pointer(base.as_ref()) && constant_value(index).is_some(),
        Expression::Member { base, .. } => {
            is_pointer(base.as_ref())
                || matches!(base.as_ref(), Expression::Dereference { pointer: inner } if is_pointer(inner.as_ref()))
        }
        _ => false,
    }
}

/// A null-guarded dereference: a guard `!p` / `p` for a register pointer p, with one arm a CONSTANT
/// and the other a safe-when-nonzero access of that p (`*p`, `p[const]`, `p->field`). Returns
/// `(pointer, hot_access, cold_constant)`. mwcc branches on `p == 0` to the cold constant and puts the
/// access in the fall-through — it cannot fold to a branchless select because dereferencing null is
/// unsafe. Int-width return only (a narrow return sign-extends even the cold constant, a byte diff).
fn guarded_null_dereference<'a>(condition: &'a Expression, value: &'a Expression, default: &'a Expression, return_type: Type) -> Option<(&'a str, &'a Expression, &'a Expression)> {
    // int/unsigned or a narrow int (char/short): the cold constant is truncated and loaded directly
    // (no over-extension) and each hot access loads at its natural width (lbz/lha/lwz).
    if !matches!(return_type, Type::Int | Type::UnsignedInt) && !is_narrow_int(return_type) {
        return None;
    }
    match condition {
        // `if (!p) return VALUE; return DEFAULT;` — p == 0 yields the constant VALUE (cold), p != 0
        // yields the DEFAULT access of p (hot).
        Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => {
            if let Expression::Variable(pointer) = operand.as_ref() {
                if constant_value(value).is_some() && accesses_pointer(default, pointer) {
                    return Some((pointer.as_str(), default, value));
                }
            }
        }
        // `if (p) return VALUE; return DEFAULT;` — p != 0 yields the VALUE access of p (hot), p == 0
        // yields the constant DEFAULT (cold).
        Expression::Variable(pointer) => {
            if accesses_pointer(value, pointer) && constant_value(default).is_some() {
                return Some((pointer.as_str(), value, default));
            }
        }
        _ => {}
    }
    None
}

/// The branchless select for a guard `if (cond) return value;` with fall-through
/// `default`. mwcc keeps the *guard value* as the in-place default, so a negated
/// guard `if (!c) ...` is compiled by stripping the `!` and swapping the arms —
/// `(c) ? default : value` — not as the bare `(!c) ? value : default` a ternary
/// would (mwcc normalizes only on the guard path, not a written ternary).
fn guard_select(condition: &Expression, value: &Expression, default: &Expression) -> Expression {
    if let Expression::Unary { operator: UnaryOperator::LogicalNot, operand } = condition {
        Expression::Conditional {
            condition: Box::new((**operand).clone()),
            when_true: Box::new(default.clone()),
            when_false: Box::new(value.clone()),
        }
    } else {
        Expression::Conditional {
            condition: Box::new(condition.clone()),
            when_true: Box::new(value.clone()),
            when_false: Box::new(default.clone()),
        }
    }
}
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::expressions::pointer_stride;
use crate::generator::*;

/// Whether a statement references (reads, writes, or takes the address of) `name`.
/// Control-flow statements are treated conservatively as referencing everything.
fn statement_references_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => expression_reads_name(target, name) || expression_reads_name(value, name),
        Statement::Assign { name: target, value } => target == name || expression_reads_name(value, name),
        Statement::Expression(expression) => expression_reads_name(expression, name),
        Statement::If { .. } | Statement::Switch { .. } | Statement::Loop { .. } | Statement::Return(_) => true,
    }
}

/// Drop locals that are never referenced anywhere and whose initializer has no side
/// effect (no call) — mwcc eliminates an unused `int s = 0;`, emitting no `li`. A
/// referenced local (read, written, or address-taken — any use of its name), or a
/// call-initialized one (whose call must still run), is kept.
fn remove_dead_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() {
        return None;
    }
    let referenced = |name: &str| -> bool {
        function.locals.iter().any(|local| {
            local.name != name && local.initializer.as_ref().map_or(false, |init| expression_reads_name(init, name))
        }) || function.statements.iter().any(|statement| statement_references_name(statement, name))
            || function.guards.iter().any(|guard| {
                expression_reads_name(&guard.condition, name) || expression_reads_name(&guard.value, name)
            })
            || function.return_expression.as_ref().map_or(false, |ret| expression_reads_name(ret, name))
    };
    let kept: Vec<LocalDeclaration> = function
        .locals
        .iter()
        .filter(|local| referenced(&local.name) || local.initializer.as_ref().map_or(false, |init| expression_has_call(init)))
        .cloned()
        .collect();
    if kept.len() == function.locals.len() {
        return None;
    }
    Some(Function { locals: kept, ..function.clone() })
}

/// Fold single-assignment, return-only locals (whose initializers make no call) into
/// the return expression, dropping them — so `int z = x + 1; g(); return z;` becomes
/// the equivalent `g(); return x + 1;`, which the parameter-preservation path compiles.
/// Only a call-making body whose locals are pure return aliases qualifies; a local
/// initialized by a call (preserved as a call result), reassigned, read by a statement,
/// or feeding control flow leaves the function unchanged (`None`).
fn inline_return_only_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() || !function_makes_call(function) || !function.guards.is_empty() {
        return None;
    }
    let return_expression = function.return_expression.as_ref()?;
    // Each local's value, with earlier locals already folded in. A call-bearing
    // initializer is a call result (preserved, not inlined), so bail.
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    for local in &function.locals {
        let initializer = local.initializer.as_ref()?;
        if expression_has_call(initializer) {
            return None;
        }
        let resolved = crate::value_tracking::substitute(initializer, &values);
        values.insert(local.name.clone(), resolved);
    }
    // The locals exist only to feed the return: every statement must be a bare
    // expression that reads none of them (a store/assign/control-flow statement is a
    // different shape).
    for statement in &function.statements {
        let Statement::Expression(expression) = statement else {
            return None;
        };
        if function.locals.iter().any(|local| expression_reads_name(expression, &local.name)) {
            return None;
        }
    }
    Some(Function {
        return_type: function.return_type,
        name: function.name.clone(),
        is_static: function.is_static,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: function.statements.clone(),
        guards: function.guards.clone(),
        return_expression: Some(crate::value_tracking::substitute(return_expression, &values)),
    })
}

/// Inline value-tracked locals that only feed a single `switch` into the switch, then recompile —
/// `int m = n + 1; switch(m) {...}` becomes `switch(n + 1) {...}`, which the switch fast path emits
/// (mwcc compiles them identically). Mirrors `inline_return_only_locals` for a switch body. Each
/// local must be an int-width (>= 32) value with a call-free initializer, read AT MOST ONCE across
/// the scrutinee/arms/default/return, so the substitution cannot duplicate a computation mwcc would
/// keep in a register. Anything outside this leaves the function unchanged (`None`) to defer honestly.
fn inline_switch_scrutinee_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() || !function.guards.is_empty() || function_makes_call(function) {
        return None;
    }
    let [Statement::Switch { scrutinee, arms, default }] = function.statements.as_slice() else {
        return None;
    };
    // Each local's value, with earlier locals folded in. A narrow local (width < 32) changes the
    // lowering (truncation/sign-extension) and a call-bearing initializer is a call result — bail.
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    for local in &function.locals {
        let initializer = local.initializer.as_ref()?;
        if expression_has_call(initializer) || local.declared_type.width() < 32 {
            return None;
        }
        values.insert(local.name.clone(), crate::value_tracking::substitute(initializer, &values));
    }
    // No inlined local may be read more than once across the whole body, so substituting it cannot
    // duplicate a computation (mwcc materializes a multiply-read value once in a register).
    for local in &function.locals {
        let mut occurrences = crate::analysis::count_name_occurrences(scrutinee, &local.name);
        for arm in arms {
            occurrences += crate::analysis::count_name_occurrences(&arm.result, &local.name);
        }
        if let Some(expression) = default {
            occurrences += crate::analysis::count_name_occurrences(expression, &local.name);
        }
        if let Some(expression) = &function.return_expression {
            occurrences += crate::analysis::count_name_occurrences(expression, &local.name);
        }
        if occurrences > 1 {
            return None;
        }
    }
    let arms = arms
        .iter()
        .map(|arm| mwcc_syntax_trees::SwitchArm { value: arm.value, result: crate::value_tracking::substitute(&arm.result, &values) })
        .collect();
    Some(Function {
        return_type: function.return_type,
        name: function.name.clone(),
        is_static: function.is_static,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: vec![Statement::Switch {
            scrutinee: crate::value_tracking::substitute(scrutinee, &values),
            arms,
            default: default.as_ref().map(|expression| crate::value_tracking::substitute(expression, &values)),
        }],
        guards: function.guards.clone(),
        return_expression: function.return_expression.as_ref().map(|expression| crate::value_tracking::substitute(expression, &values)),
    })
}

/// Tally reads of each tracked local in `expression` toward its current value-version's
/// running count, returning true if a computed (non-Variable) version would then be read at
/// a second materialization site. mwcc computes such a value once and keeps it in a
/// register; inlining would duplicate the computation, so the fold must bail. A Variable
/// value is register-resident and free to re-read any number of times.
fn fold_would_duplicate(
    expression: &Expression,
    local_names: &std::collections::HashSet<&str>,
    values: &std::collections::HashMap<String, Expression>,
    read_count: &mut std::collections::HashMap<String, usize>,
) -> bool {
    for &name in local_names {
        let occurrences = crate::analysis::count_name_occurrences(expression, name);
        if occurrences == 0 {
            continue;
        }
        let total = read_count.entry(name.to_string()).or_insert(0);
        *total += occurrences;
        let computed = values.get(name).is_some_and(|value| !matches!(value, Expression::Variable(_)));
        if computed && *total >= 2 {
            return true;
        }
    }
    false
}

/// Fold a function's value-tracked locals into its stores and trailing return, then
/// recompile — `int x = a; gi = x; x = b; gj = x;` becomes `gi = a; gj = b;`, and `int x =
/// a; gi = x; return x;` becomes `gi = a; return a;`. The store paths (or, when mwcc would
/// latency-schedule the stores, the un-schedulable-store deferral) own the cleaned body. The
/// locals exist only to feed the stores and the return, so tracking their values
/// sequentially and substituting eliminates them. Bails on a call (in the body or a value —
/// a side effect to preserve), a guard, a non-store/assign statement, a store into a local,
/// a local that survives the substitution, or a fold that would duplicate a computed value
/// at 2+ sites (mwcc keeps it in one register — fold_would_duplicate). A store-free body
/// (pure dead-local, or pure return-folding) is left to the value-tracking path.
fn inline_store_bearing_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() || function_makes_call(function) || !function.guards.is_empty() {
        return None;
    }
    // A NARROWING narrow local (`char c = a;` for a wider `a`) must not inline: substituting
    // the wider value drops the truncation + sign-extension — `char c = a; gi = c;` would
    // store the full int instead of `(int)(char)a`. Decline so the function defers honestly
    // on the normal path rather than emitting the raw value.
    let variable_width = |name: &str| -> Option<u32> {
        function
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .map(|parameter| parameter.parameter_type.width() as u32)
            .or_else(|| {
                function
                    .locals
                    .iter()
                    .find(|local| local.name == name)
                    .map(|local| local.declared_type.width() as u32)
            })
    };
    for local in &function.locals {
        if (local.declared_type.width() as u32) < 32 {
            if let Some(Expression::Variable(initializer_name)) = &local.initializer {
                if variable_width(initializer_name).is_some_and(|width| width > local.declared_type.width() as u32) {
                    return None;
                }
            }
        }
    }
    let local_names: std::collections::HashSet<&str> =
        function.locals.iter().map(|local| local.name.as_str()).collect();
    // Each local's current value, earlier folds applied. Seed from initializers (a call-
    // bearing initializer is a call result to preserve, not inline). `read_count` tracks how
    // many times each local's CURRENT value-version is read, to reject duplicating a
    // computation; reassignment resets it.
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    let mut read_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for local in &function.locals {
        let Some(initializer) = &local.initializer else { continue };
        if expression_has_call(initializer) || fold_would_duplicate(initializer, &local_names, &values, &mut read_count) {
            return None;
        }
        values.insert(local.name.clone(), crate::value_tracking::substitute(initializer, &values));
    }
    let mut new_statements = Vec::new();
    for statement in &function.statements {
        match statement {
            Statement::Assign { name, value } => {
                if !local_names.contains(name.as_str()) || expression_has_call(value) {
                    return None;
                }
                if fold_would_duplicate(value, &local_names, &values, &mut read_count) {
                    return None;
                }
                values.insert(name.clone(), crate::value_tracking::substitute(value, &values));
                read_count.insert(name.clone(), 0);
            }
            Statement::Store { target, value } => {
                if expression_has_call(value) || expression_has_call(target) {
                    return None;
                }
                // A store INTO a local is a different shape — we only fold locals that feed
                // memory stores, not locals that are themselves store targets.
                if let Expression::Variable(name) = target {
                    if local_names.contains(name.as_str()) {
                        return None;
                    }
                }
                if fold_would_duplicate(target, &local_names, &values, &mut read_count)
                    || fold_would_duplicate(value, &local_names, &values, &mut read_count)
                {
                    return None;
                }
                new_statements.push(Statement::Store {
                    target: crate::value_tracking::substitute(target, &values),
                    value: crate::value_tracking::substitute(value, &values),
                });
            }
            _ => return None,
        }
    }
    if let Some(return_expression) = &function.return_expression {
        if fold_would_duplicate(return_expression, &local_names, &values, &mut read_count) {
            return None;
        }
    }
    // A store-free body (a pure dead-local, or pure return-folding) is the value-tracking
    // path's job, not ours.
    if new_statements.is_empty() {
        return None;
    }
    let folded_return = function
        .return_expression
        .as_ref()
        .map(|expression| crate::value_tracking::substitute(expression, &values));
    // Every local must be fully folded away — none may survive in a resulting store or the
    // return (e.g. a local whose aggregate or address use could not be substituted).
    let survives = |expression: &Expression| local_names.iter().any(|name| expression_reads_name(expression, name));
    for statement in &new_statements {
        if let Statement::Store { target, value } = statement {
            if survives(target) || survives(value) {
                return None;
            }
        }
    }
    if folded_return.as_ref().is_some_and(survives) {
        return None;
    }
    Some(Function {
        return_type: function.return_type,
        name: function.name.clone(),
        is_static: function.is_static,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: new_statements,
        guards: function.guards.clone(),
        return_expression: folded_return,
    })
}

/// A single local whose value is exactly one call, consumed once — either stored directly to
/// a global (`int x = foo(...); gi = x;`) or returned (`int x = foo(...); return x;`). mwcc
/// leaves the call result in r3; `gi = foo(...)` and `return foo(...)` are both already
/// byte-exact, and the result is not live across any other call, so it needs no callee-save.
/// Inline the local and recompile. This is the trivial entry into value-tracking-with-calls;
/// a second call, a second use of the result, the result fused with arithmetic, or any other
/// statement all need the callee-saved allocator and fall through to it. (Kept separate from
/// inline_store_bearing_locals, which bails on any call at its entry.)
fn inline_single_call_result(function: &Function) -> Option<Function> {
    if !function.guards.is_empty() || function.locals.len() != 1 {
        return None;
    }
    let local_name = function.locals[0].name.as_str();
    // The local's value is exactly one call, set once — by the initializer xor a single
    // assignment — and the call must not read the local itself.
    let mut call_value: Option<Expression> = None;
    if let Some(initializer) = &function.locals[0].initializer {
        if !matches!(initializer, Expression::Call { .. }) || expression_reads_name(initializer, local_name) {
            return None;
        }
        call_value = Some(initializer.clone());
    }
    let mut store: Option<(Expression, Expression)> = None;
    for statement in &function.statements {
        match statement {
            Statement::Assign { name, value } if name == local_name => {
                if call_value.is_some()
                    || !matches!(value, Expression::Call { .. })
                    || expression_reads_name(value, local_name)
                {
                    return None;
                }
                call_value = Some(value.clone());
            }
            // The result is consumed by exactly one store, whose target does not read the
            // local (only its value may).
            Statement::Store { target, value } => {
                if store.is_some() || expression_reads_name(target, local_name) {
                    return None;
                }
                store = Some((target.clone(), value.clone()));
            }
            _ => return None,
        }
    }
    let call_value = call_value?;
    // The result is consumed in exactly one place and read EXACTLY ONCE there — a call read
    // twice would call twice. Substitute the call into that single use (`gi = x + 1;` ->
    // `gi = foo(a) + 1;`); the re-dispatch is byte-exact (call fused with a constant) or
    // defers (a value live across the call), never a diff.
    let occurrences = |expression: &Expression| crate::analysis::count_name_occurrences(expression, local_name);
    let mut values = std::collections::HashMap::new();
    values.insert(local_name.to_string(), call_value);
    let (statements, return_expression) = match &store {
        // Store sink: a void function with no return, the local consumed once in the value.
        Some((target, value)) if function.return_type == Type::Void && function.return_expression.is_none() => {
            if occurrences(value) != 1 {
                return None;
            }
            (vec![Statement::Store { target: target.clone(), value: crate::value_tracking::substitute(value, &values) }], None)
        }
        // Return sink: no stores, the trailing return consumes the local once.
        None => {
            let return_expression = function.return_expression.as_ref()?;
            if occurrences(return_expression) != 1 {
                return None;
            }
            (Vec::new(), Some(crate::value_tracking::substitute(return_expression, &values)))
        }
        _ => return None,
    };
    Some(Function {
        return_type: function.return_type,
        name: function.name.clone(),
        is_static: function.is_static,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements,
        guards: Vec::new(),
        return_expression,
    })
}

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
            let stride = pointer_stride(parameter.parameter_type);
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed, width: parameter.parameter_type.width(), pointee, stride },
            );
        }
        Ok(())
    }

    /// Emit a function involving a long long (64-bit) value, held in a general-register PAIR
    /// (`r3:r4` = high:low). Only a narrow set of shapes is modeled; the rest defer rather than
    /// fall through to the 32-bit codegen (which emits a single-register result for a 64-bit value).
    fn emit_long_long(&mut self, function: &Function) -> Compilation<()> {
        // Long-long LOCALS (which need pair spills), guards, and statements are not modeled yet.
        if !function.locals.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
        }
        let high = Eabi::general_result().number; // r3 — the result HIGH word
        let low = high + 1; //                       r4 — the result LOW word
        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void long long function needs a return value"))?;
        let any_long_long_parameter = function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::LongLong | Type::UnsignedLongLong));

        // ===== No long-long PARAMETERS: a long-long RETURN from a constant or a widened 32-bit value.
        if !any_long_long_parameter {
            if !matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong) {
                return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
            }
            // (a) A 64-bit integer CONSTANT — `li low,LOW ; li high,HIGH` (LOW word first, as mwcc
            // emits it). Restricted to words that load with a single `li`.
            if let Some(value) = crate::analysis::constant_value(return_expression) {
                let low_word = value as i32 as i64;
                let high_word = value >> 32;
                if i16::try_from(low_word).is_err() || i16::try_from(high_word).is_err() {
                    return Err(Diagnostic::error("a wide long long constant needs lis/ori (roadmap)"));
                }
                self.load_integer_constant(low, low_word);
                self.load_integer_constant(high, high_word);
                self.emit_epilogue_and_return();
                return Ok(());
            }
            // (b) Widen a 32-bit int/unsigned FIRST PARAMETER. It arrives in r3 (= result HIGH), so
            // copy it to LOW, then fill HIGH with its sign (`srawi`) or zero (`li`). A NARROW source
            // (short/char) re-extends differently and defers.
            if let Expression::Variable(name) = return_expression {
                if function.parameters.first().is_some_and(|parameter| &parameter.name == name) {
                    let parameter_type = function.parameters[0].parameter_type;
                    if matches!(parameter_type, Type::Int | Type::UnsignedInt) {
                        self.output.instructions.push(Instruction::move_register(low, high));
                        if parameter_type.is_signed() {
                            self.output
                                .instructions
                                .push(Instruction::ShiftRightAlgebraicImmediate { a: high, s: high, shift: 31 });
                        } else {
                            self.load_integer_constant(high, 0);
                        }
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            return Err(Diagnostic::error("this long long return shape is not modeled yet (roadmap)"));
        }

        // ===== Long-long PARAMETERS present. Allocate GPR argument registers per the EABI: each
        // int-like param takes one GPR; each long-long param an odd-start GPR pair (aligning up if
        // the next GPR is even), so `f(int x, long long a)` puts x in r3 and a in r5:r6. A float/
        // double/struct param alongside a long long (FPRs or aggregates) and an argument list that
        // overflows r3..r10 both defer.
        const LAST_GENERAL_ARGUMENT: u8 = Eabi::FIRST_GENERAL_ARGUMENT + 7; // r10
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut param_pair: std::collections::HashMap<&str, (u8, Option<u8>)> = std::collections::HashMap::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::LongLong | Type::UnsignedLongLong => {
                    if next_general % 2 == 0 {
                        next_general += 1; // a long-long pair starts on an odd register
                    }
                    if next_general + 1 > LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("a long-long argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, Some(next_general + 1)));
                    next_general += 2;
                }
                Type::Int | Type::UnsignedInt | Type::Short | Type::UnsignedShort | Type::Char | Type::UnsignedChar
                | Type::Pointer(_) | Type::StructPointer { .. } => {
                    if next_general > LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("an integer argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, None));
                    next_general += 1;
                }
                _ => return Err(Diagnostic::error("a float/double/struct parameter alongside a long long is not modeled yet (roadmap)")),
            }
        }

        // (c) TRUNCATE a long-long param to int/unsigned — `(int)a` or implicit — is its LOW word:
        // `mr r3, low(a)`.
        if matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            let truncated = match return_expression {
                Expression::Cast { target_type: Type::Int | Type::UnsignedInt, operand } => operand.as_ref(),
                other => other,
            };
            if let Expression::Variable(name) = truncated {
                if let Some(&(_, Some(low_register))) = param_pair.get(name.as_str()) {
                    self.output.instructions.push(Instruction::move_register(high, low_register));
                    self.emit_epilogue_and_return();
                    return Ok(());
                }
            }
            return Err(Diagnostic::error("this long long truncation is not modeled yet (roadmap)"));
        }
        if !matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong) {
            return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
        }

        // (d) RETURN a long-long param: move its pair into the result pair (a bare `blr` when it is
        // already there — the first parameter). mwcc moves LOW then HIGH (`mr r4,r6 ; mr r3,r5`).
        if let Expression::Variable(name) = return_expression {
            if let Some(&(parameter_high, Some(parameter_low))) = param_pair.get(name.as_str()) {
                if parameter_high != high {
                    self.output.instructions.push(Instruction::move_register(low, parameter_low));
                    self.output.instructions.push(Instruction::move_register(high, parameter_high));
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }

        // (e) ADD / SUBTRACT two long-long params into the result pair; the LOW word carries into
        // HIGH: `addc r4,r4,r6 ; adde r3,r3,r5` or `subfc r4,r6,r4 ; subfe r3,r5,r3`.
        if let Expression::Binary { operator, left, right } = return_expression {
            if let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) {
                if let (Some(&(left_high, Some(left_low))), Some(&(right_high, Some(right_low)))) =
                    (param_pair.get(left_name.as_str()), param_pair.get(right_name.as_str()))
                {
                    match operator {
                        BinaryOperator::Add => {
                            self.output.instructions.push(Instruction::AddCarrying { d: low, a: left_low, b: right_low });
                            self.output.instructions.push(Instruction::AddExtended { d: high, a: left_high, b: right_high });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        // subfc rD,rA,rB = rB - rA, so the minuend (left) is `b` and subtrahend (right) is `a`.
                        BinaryOperator::Subtract => {
                            self.output.instructions.push(Instruction::SubtractFromCarrying { d: low, a: right_low, b: left_low });
                            self.output.instructions.push(Instruction::SubtractFromExtended { d: high, a: right_high, b: left_high });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        // (f) ADD/SUBTRACT a small CONSTANT to a single long-long parameter. mwcc materializes the
        // 64-bit constant — its LOW word into the next free GPR (r5) and its HIGH word into r0, or
        // just r0 when both words are equal — then `addc`/`adde`. `a - C` lowers as `a + (-C)`.
        // Restricted to a single long-long parameter (so a == result == r3:r4 and r5 is free) and
        // li-sized constant words; a wider constant or a second parameter (dead-register reuse)
        // defers.
        if function.parameters.len() == 1 {
            if let Expression::Binary { operator, left, right } = return_expression {
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), crate::analysis::constant_value(right)) {
                        if param_pair.get(name.as_str()).is_some_and(|&(_, low_word)| low_word.is_some()) {
                            let value = if *operator == BinaryOperator::Subtract { constant.wrapping_neg() } else { constant };
                            let low_word = value as i32 as i64;
                            let high_word = value >> 32;
                            if i16::try_from(low_word).is_ok() && i16::try_from(high_word).is_ok() {
                                if low_word == high_word {
                                    self.load_integer_constant(GENERAL_SCRATCH, low_word);
                                    self.output.instructions.push(Instruction::AddCarrying { d: low, a: low, b: GENERAL_SCRATCH });
                                    self.output.instructions.push(Instruction::AddExtended { d: high, a: high, b: GENERAL_SCRATCH });
                                } else {
                                    let low_constant_register = high + 2; // r5 — the next free GPR after r3:r4
                                    self.load_integer_constant(low_constant_register, low_word);
                                    self.load_integer_constant(GENERAL_SCRATCH, high_word);
                                    self.output.instructions.push(Instruction::AddCarrying { d: low, a: low, b: low_constant_register });
                                    self.output.instructions.push(Instruction::AddExtended { d: high, a: high, b: GENERAL_SCRATCH });
                                }
                                self.emit_epilogue_and_return();
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"))
    }

    /// Emit the whole function body, including its `blr`(s).
    pub(crate) fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        // Drop never-referenced, side-effect-free locals (an unused `int s = 0;`) — mwcc
        // emits nothing for them — then recompile the cleaned function.
        if let Some(cleaned) = remove_dead_locals(function) {
            return self.evaluate_body(&cleaned);
        }
        // Returning a struct BY VALUE (`struct S f(...) { return s; }`) uses the struct-return
        // ABI — a small struct in r3:r4, a larger one via a hidden pointer argument — which is
        // not modeled. Defer rather than emit a bare `blr` that drops the result (a miscompile:
        // the caller would read the input pointer / stale registers as the returned struct).
        if matches!(function.return_type, Type::Struct { .. }) {
            return Err(Diagnostic::error("returning a struct by value is not supported yet (roadmap)"));
        }
        // A long long (64-bit) value lives in a general-register PAIR — r3:r4 is high:low. Route
        // every long-long-involved function to the dedicated handler so none falls through to the
        // 32-bit codegen (which would emit a single-register result for a 64-bit value — wrong
        // bytes). The handler models a narrow set of shapes and defers the rest.
        if matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong)
            || function.parameters.iter().any(|parameter| matches!(parameter.parameter_type, Type::LongLong | Type::UnsignedLongLong))
            || function.locals.iter().any(|local| matches!(local.declared_type, Type::LongLong | Type::UnsignedLongLong))
        {
            return self.emit_long_long(function);
        }
        // `loc = …; return loc` where `loc` is a VARIABLE-INDEXED access (`p[i]`) or a GLOBAL —
        // mwcc reuses the scaled index it already computed (`slwi` once) or the just-stored value,
        // but ours recomputes the index (`slwi` twice) or reloads the global, a byte-different
        // sequence. Defer. (A deref `*p`, a member `s->x`, a const index `p[0]`, and a
        // register param/local are byte-exact and unaffected.)
        if let Some(return_expression) = &function.return_expression {
            for statement in &function.statements {
                if let Statement::Store { target, .. } = statement {
                    if structurally_equal(target, return_expression) {
                        let recomputes_address = matches!(target, Expression::Index { index, .. } if constant_value(index).is_none())
                            || matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
                        if recomputes_address {
                            return Err(Diagnostic::error("storing to a variable-indexed or global location then returning it recomputes the address (roadmap)"));
                        }
                    }
                }
            }
        }
        // `global = const; return <const or global>` — mwcc's scheduler computes the return value
        // (a `li` for a constant, an SDA `lwz` for a global) BEFORE the global constant store; ours
        // emits the store first. A param return (already in r3) or a deref/index return is
        // byte-exact and unaffected, as is a non-constant or non-global store.
        if let Some(return_expression) = &function.return_expression {
            let return_is_const_or_global = constant_value(return_expression).is_some()
                || matches!(return_expression, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
            if return_is_const_or_global {
                for statement in &function.statements {
                    if let Statement::Store { target, value } = statement {
                        if constant_value(value).is_some()
                            && matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()))
                        {
                            return Err(Diagnostic::error("a global constant store scheduled around a const/global return is not modeled (roadmap)"));
                        }
                    }
                }
            }
        }
        // A function that takes the address of a variable lowers it to a stack
        // slot (frame-resident); this takes over the whole body. Checked first,
        // since an address-taken variable cannot be value-tracked in a register.
        if self.try_frame_resident(function)? {
            return Ok(());
        }
        // A counting `for (i = 0; i < bound; i++)` loop owns its single local
        // counter, so it is checked before the value-tracking path claims it.
        if self.try_for_counter(function)? {
            return Ok(());
        }
        // A leaf non-counting `while`/`do-while` whose body is pure in-place increments
        // (`while (*p) p++;`) lowers to the rotated form; claimed before value-tracking since the
        // loop-carried increment must emit in place.
        if self.try_emit_increment_while(function)? {
            return Ok(());
        }
        // `T y; if (c) y = A; else y = B; return y;` — both arms assign the returned
        // local, so the whole body is the select `return (c) ? A : B`.
        if self.try_conditional_assign(function)? {
            return Ok(());
        }
        // `T y = INIT; if (c) y = NEW; return y;` (no else), constant arms — mwcc lowers the
        // conditional ASSIGN as an early-return branch form (NOT the select/branchless idiom).
        if self.try_conditional_assign_initialized(function)? {
            return Ok(());
        }
        // A function's value-tracked locals are folded into its stores and trailing return,
        // then recompiled — `int x = a; gi = x; x = b; gj = x;` becomes `gi = a; gj = b;`,
        // and `int x = a; gi = x; return x;` becomes `gi = a; return a;`. The store paths
        // (or the un-schedulable-store deferral) own the cleaned body. Checked before the
        // value-tracking path, which cannot fold a void function's store-feeding locals.
        if let Some(inlined) = inline_store_bearing_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // `int x = foo(...); gi = x;` / `int x = foo(...); return x;` — a single-use call
        // result stored directly or returned. The result lives in r3 and is not live across
        // another call, so both are byte-exact; inline the local. A second call or use
        // defers to the callee-saved allocator.
        if let Some(inlined) = inline_single_call_result(function) {
            return self.evaluate_body(&inlined);
        }
        // Value-tracked locals (reassignment, multiple locals) are inlined into the
        // return expression and compiled there; this takes over the whole body when
        // it applies, leaving the straight-line paths below byte-identical.
        if self.try_value_tracking(function)? {
            return Ok(());
        }
        // Fold single-assignment, return-only locals (no call in their initializers)
        // into the return, then recompile — `int z = x + 1; g(); return z;` becomes the
        // equivalent `g(); return x + 1;`, which the parameter-preservation path emits.
        if let Some(inlined) = inline_return_only_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A value-tracked local feeding a single switch's scrutinee/arms inlines into the switch and
        // recompiles, so `int m = n + 1; switch(m)` lowers like the direct `switch(n + 1)`.
        if let Some(inlined) = inline_switch_scrutinee_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A leaf void body that is purely constant stores of one repeated value
        // (struct/array zeroing) materializes the value once and reuses it.
        if self.try_constant_store_fill(function)? {
            return Ok(());
        }
        // Two computed-value stores to distinct SDA globals: mwcc overlaps the two value
        // computations (both into registers, then both stores), which the sequential path
        // does not. The allocator places the first value off the scratch (live across the
        // second), the second into r0.
        if self.try_computed_store_fill(function)? {
            return Ok(());
        }
        // The same overlap with one computed value and one register-leaf value (`gi=a+1;
        // gj=b;`): the leaf is stored first (ready), the computed second.
        if self.try_mixed_store_fill(function)? {
            return Ok(());
        }
        // Three+ stores of register leaves with a single constant interspersed (`gi=a;
        // gj=b; gk=5;`): the constant's `li` is hoisted and the stores keep source order
        // (a leading constant swaps off the latency slot).
        if self.try_leaf_constant_fill(function)? {
            return Ok(());
        }
        // Multiple stores where a value loads a float/double global reschedule the loads
        // (mwcc loads the global once and reuses it across the stores); not modeled, so
        // DEFER rather than emit a redundant load per store. A single such store (`gf =
        // gg;`) needs no scheduling and stays byte-exact.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
        {
            let loads_float_global = |generator: &Self, value: &Expression| {
                matches!(value, Expression::Variable(name)
                    if !generator.locations.contains_key(name.as_str())
                        && matches!(generator.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            };
            let all_stores = function.statements.iter().all(|statement| matches!(statement, Statement::Store { .. }));
            let any_float_global = function
                .statements
                .iter()
                .any(|statement| matches!(statement, Statement::Store { value, .. } if loads_float_global(self, value)));
            if all_stores && any_float_global {
                return Err(Diagnostic::error("multiple stores loading a float global need the load scheduler (roadmap)"));
            }
        }
        // Un-schedulable multi-store: a body whose statements are 2+ stores to SDA integer
        // globals that the fills above did not absorb (a trailing return, if any, is
        // separate). mwcc latency-schedules these (load/computation hoisting, constant-`li`
        // slot fill); the normal sequential emission would not reproduce that, so DEFER
        // rather than ship wrong bytes. Only an all-distinct-leaf run (no computation to
        // schedule, no dead store) stays byte-exact on the normal path, so let that through.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
            && self.behavior.global_addressing == GlobalAddressing::SmallData
        {
            let mut targets = Vec::new();
            let mut all_leaves = true;
            let mut all_sda_integer_stores = true;
            for statement in &function.statements {
                let Statement::Store { target: Expression::Variable(name), value } = statement else {
                    all_sda_integer_stores = false;
                    break;
                };
                match self.globals.get(name.as_str()) {
                    Some(global_type) if !matches!(global_type, Type::Float | Type::Double) => targets.push(name.as_str()),
                    _ => {
                        all_sda_integer_stores = false;
                        break;
                    }
                }
                if !matches!(value, Expression::Variable(leaf) if !self.globals.contains_key(leaf.as_str())) {
                    all_leaves = false;
                }
            }
            if all_sda_integer_stores {
                let distinct = {
                    let mut sorted = targets.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    sorted.len() == targets.len()
                };
                if !all_leaves || !distinct {
                    return Err(Diagnostic::error("a run of stores that mwcc latency-schedules needs the scheduler (roadmap)"));
                }
            }
        }
        // A `do { …calls… } while (--counter);` loop: the counter goes in r31
        // (callee-saved), the body branches back, and the decrement-and-test is a
        // single `addic.`/`bne`.
        if self.try_do_while_counter(function)? {
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
                && !then_body.is_empty()
                // A straight-line body (calls/stores, no nested control flow); a value
                // read across one of its calls would need callee-saving, so defer it.
                && then_body.iter().all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The if's join label advances mwcc's anonymous-`@N` counter by 2.
                self.output.anonymous_label_bump = 2;
                self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
                self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // mwcc fills the mflr->LR-store latency slot with the condition test only
                // when it is a bare compare (a register operand). A member/complex
                // condition loads into r0, which would clobber the just-saved LR, so the
                // LR store must come first — otherwise it would save the loaded value, not
                // the return address.
                // mwcc fills the mflr->LR-store latency slot with the FIRST condition
                // instruction when it does not write r0 (a compare, or a float load/
                // compare targeting cr/FP), issuing the LR store right after it (e.g.
                // `lfs f0; stw r0,20; fcmpo`). An integer load / rlwinm. / extsb. into r0
                // would clobber the saved LR, so the store precedes the whole condition.
                let first_writes_r0 = self.output.instructions.get(condition_start).map_or(false, |instruction| {
                    match instruction {
                        // Compares and float/cr ops write cr0/an FPR, not a GPR.
                        Instruction::CompareWord { .. }
                        | Instruction::CompareWordImmediate { .. }
                        | Instruction::CompareLogicalWord { .. }
                        | Instruction::CompareLogicalWordImmediate { .. }
                        | Instruction::FloatCompareOrdered { .. }
                        | Instruction::FloatCompareUnordered { .. }
                        | Instruction::LoadFloatSingle { .. }
                        | Instruction::LoadFloatSingleIndexed { .. }
                        | Instruction::LoadFloatDouble { .. }
                        | Instruction::LoadFloatDoubleIndexed { .. }
                        | Instruction::ConditionRegisterOr { .. } => false,
                        // A narrow extension into a non-r0 GPR — `extsh r3,r3`, the first
                        // operand of a two-operand narrow compare — leaves the saved LR in r0
                        // intact, so the store still fills the slot after it. Extending into
                        // r0 (a narrow leaf against a constant) clobbers it: store first.
                        Instruction::ExtendSignByte { a, .. }
                        | Instruction::ExtendSignByteRecord { a, .. }
                        | Instruction::ExtendSignHalfword { a, .. }
                        | Instruction::ExtendSignHalfwordRecord { a, .. }
                        | Instruction::ClearLeftImmediate { a, .. }
                        | Instruction::ClearLeftImmediateRecord { a, .. } => *a == 0,
                        // Any other first instruction writes a GPR (a load into r0, rlwinm.).
                        _ => true,
                    }
                });
                let lr_position = if first_writes_r0 { condition_start } else { condition_start + 1 };
                self.output.instructions.insert(lr_position, Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                // The insert shifts the condition instructions at/after it down by one, so
                // their relocations (a global condition's SDA21 reloc) must shift too.
                for relocation in &mut self.output.relocations {
                    if relocation.instruction_index >= lr_position {
                        relocation.instruction_index += 1;
                    }
                }
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
        // A non-leaf `if (c) { then } else { else }` with straight-line bodies: the
        // condition test schedules into the prologue, `beq` jumps to the else body,
        // the then body falls through to an unconditional `b` over the else body to
        // the shared epilogue.
        if let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() {
            if function_makes_call(function)
                && function.return_type == Type::Void
                && function.guards.is_empty()
                && !then_body.is_empty()
                && !else_body.is_empty()
                && then_body.iter().chain(else_body).all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The else branch and join label advance mwcc's anonymous-`@N` counter.
                self.output.anonymous_label_bump = 3;
                self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
                self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // mwcc fills the mflr->LR-store latency slot with the condition test only
                // when it is a bare compare (a register operand). A member/complex
                // condition loads into r0, which would clobber the just-saved LR, so the
                // LR store must come first — otherwise it would save the loaded value, not
                // the return address.
                // mwcc fills the mflr->LR-store latency slot with the FIRST condition
                // instruction when it does not write r0 (a compare, or a float load/
                // compare targeting cr/FP), issuing the LR store right after it (e.g.
                // `lfs f0; stw r0,20; fcmpo`). An integer load / rlwinm. / extsb. into r0
                // would clobber the saved LR, so the store precedes the whole condition.
                let first_writes_r0 = self.output.instructions.get(condition_start).map_or(false, |instruction| {
                    match instruction {
                        // Compares and float/cr ops write cr0/an FPR, not a GPR.
                        Instruction::CompareWord { .. }
                        | Instruction::CompareWordImmediate { .. }
                        | Instruction::CompareLogicalWord { .. }
                        | Instruction::CompareLogicalWordImmediate { .. }
                        | Instruction::FloatCompareOrdered { .. }
                        | Instruction::FloatCompareUnordered { .. }
                        | Instruction::LoadFloatSingle { .. }
                        | Instruction::LoadFloatSingleIndexed { .. }
                        | Instruction::LoadFloatDouble { .. }
                        | Instruction::LoadFloatDoubleIndexed { .. }
                        | Instruction::ConditionRegisterOr { .. } => false,
                        // A narrow extension into a non-r0 GPR — `extsh r3,r3`, the first
                        // operand of a two-operand narrow compare — leaves the saved LR in r0
                        // intact, so the store still fills the slot after it. Extending into
                        // r0 (a narrow leaf against a constant) clobbers it: store first.
                        Instruction::ExtendSignByte { a, .. }
                        | Instruction::ExtendSignByteRecord { a, .. }
                        | Instruction::ExtendSignHalfword { a, .. }
                        | Instruction::ExtendSignHalfwordRecord { a, .. }
                        | Instruction::ClearLeftImmediate { a, .. }
                        | Instruction::ClearLeftImmediateRecord { a, .. } => *a == 0,
                        // Any other first instruction writes a GPR (a load into r0, rlwinm.).
                        _ => true,
                    }
                });
                let lr_position = if first_writes_r0 { condition_start } else { condition_start + 1 };
                self.output.instructions.insert(lr_position, Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                // The insert shifts the condition instructions at/after it down by one, so
                // their relocations (a global condition's SDA21 reloc) must shift too.
                for relocation in &mut self.output.relocations {
                    if relocation.instruction_index >= lr_position {
                        relocation.instruction_index += 1;
                    }
                }
                let branch_to_else = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let branch_to_join = self.output.instructions.len();
                self.output.instructions.push(Instruction::Branch { target: 0 });
                let else_label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_to_else] {
                    *target = else_label;
                }
                for statement in else_body {
                    self.emit_statement(statement)?;
                }
                let join_label = self.output.instructions.len();
                if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
                    *target = join_label;
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // A non-leaf function led by `if (c) { …calls…; return X; }` with a
        // continuation that supplies the other exit: mwcc schedules the condition
        // test into the prologue, the early return materializes X and branches to a
        // SHARED epilogue, and the continuation falls into that same epilogue.
        if self.try_non_leaf_if_first_early_return(function)? {
            return Ok(());
        }
        // A function that calls is non-leaf: save the link register around a 16-byte
        // frame before doing anything else.
        let mut lr_store_index: Option<usize> = None;
        if function_makes_call(function) {
            if !function.guards.is_empty() {
                return Err(Diagnostic::error("calls combined with guards not yet supported"));
            }
            // Parameters live across the call go in callee-saved registers (r31
            // descending), saved in the prologue and reloaded in the epilogue.
            if self.try_callee_saved(function)? {
                return Ok(());
            }
            if self.try_callee_saved_call_result(function)? {
                return Ok(());
            }
            if self.try_callee_saved_computed_local(function)? {
                return Ok(());
            }
            // A parameter passed to several calls in turn (`g(x); h(x);`) — saved in r31,
            // the first call uses the incoming register, later calls restore from r31.
            if self.try_callee_saved_call_args(function)? {
                return Ok(());
            }
            // `return f(...) + x;` — a live parameter combined with a call's result in the return.
            if self.try_callee_saved_call_combine(function)? {
                return Ok(());
            }
            // Byte-exact-or-defer: a value (parameter or register local) read after a
            // call is read from a register the call clobbered. mwcc preserves it in a
            // callee-saved register (r31…) — multi-value/local cases are the next
            // step; until then DEFER rather than emit a read of the clobbered register.
            if reads_value_across_call(function) {
                return Err(Diagnostic::error("a value live across a call needs the callee-saved register allocator (roadmap)"));
            }
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            lr_store_index = Some(self.output.instructions.len() - 1);
        }

        // Body statements (stores, calls) run first.
        let statement_count = function.statements.len();
        for (index, statement) in function.statements.iter().enumerate() {
            // A trailing `if (c) { body }` in a leaf void function: the false path
            // is the function exit, so it is a conditional return, then the body,
            // then the normal `blr`. (Non-leaf needs a forward branch to the
            // epilogue, and a non-final if needs to skip forward — both deferred.)
            if let Statement::If { condition, then_body, else_body } = statement {
                // A leaf if whose then-body is at most one statement then an early
                // `return`, with a continuation after it (more statements or the
                // trailing return): forward-branch over the body, the return is an
                // exit, and the branch lands on the continuation. Two or more
                // leading statements (constant stores mwcc would interleave) need
                // the scheduler. With no continuation (a trailing void if) the
                // false path is the immediate exit, which is a `beqlr` form — that
                // and the multi-statement case defer.
                let has_continuation = index + 1 < statement_count || function.return_expression.is_some();
                // A trailing void `if (c) { stmt; return; }` (nothing after): the
                // `return;` coincides with the function exit, so drop it and use
                // the conditional-return (`beqlr`) form of a plain trailing if.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && !has_continuation
                    && function.return_type == Type::Void
                    && then_body.len() == 2
                    && matches!(then_body.last(), Some(Statement::Return(None)))
                {
                    self.emit_trailing_if(condition, &then_body[..1], else_body)?;
                    continue;
                }
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && then_body.len() <= 2
                    && has_continuation
                    && matches!(then_body.last(), Some(Statement::Return(_)))
                    // A store before a VALUE return must be INTERLEAVED with the return-value
                    // computation the way mwcc's scheduler does (`li r0,V; li r3,R; stw r0`, not
                    // `li r0,V; stw r0; li r3,R`) — that needs the keystone scheduler (#20), so
                    // defer it. A valueless `return;` has no value to interleave (store + bare
                    // epilogue is byte-exact), and a value-tracked Assign emits nothing here, so
                    // both of those stay byte-exact.
                    && (matches!(then_body.last(), Some(Statement::Return(None)))
                        || then_body[..then_body.len() - 1].iter().all(|statement| matches!(statement, Statement::Assign { .. })))
                {
                    self.emit_if_early_return(condition, then_body, function.return_type)?;
                    continue;
                }
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

        // Hoist a leading register move from the body's statements (a call's argument
        // setup) into the prologue's mflr->LR-store slot.
        self.hoist_leading_arg_moves(lr_store_index);

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
            // Guard + single value-tracked local, zero-select: `int x = a+1; if (c) return 0;
            // return x;` (or `if (c) return x; return 0;`). mwcc materializes the local in
            // the result register but SCHEDULES the materialization into the select's
            // neg->or latency slot — `neg r0,c; addi r3,a,1; or r0,r0,c; srawi r0,31; and/
            // andc r3,r3,r0` (the addi AFTER the leading neg). Emit that interleave directly:
            // leading neg, the local, then the mask combine. Restricted to a single-op
            // integer local, a leaf condition, no statements, and exactly one arm the
            // constant 0 (the other the local).
            if let ([local], [guard]) = (function.locals.as_slice(), function.guards.as_slice()) {
                let zero_is_then = matches!(guard.value, Expression::IntegerLiteral(0));
                let zero_is_else = matches!(return_expression, Expression::IntegerLiteral(0));
                let local_is_other = (zero_is_then && matches!(return_expression, Expression::Variable(name) if *name == local.name))
                    || (zero_is_else && matches!(&guard.value, Expression::Variable(name) if *name == local.name));
                let condition_register = leaf_name(&guard.condition).and_then(|name| self.lookup_general(name));
                let initializer = local.initializer.as_ref();
                if local_is_other
                    && function.statements.is_empty()
                    && initializer.is_some_and(|init| self.is_single_op_register_value(init))
                    && class_of(local.declared_type)? == ValueClass::General
                {
                    if let (Some(condition_register), Some(initializer)) = (condition_register, initializer) {
                        self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
                        self.evaluate(initializer, local.declared_type, result)?;
                        self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
                        self.output.instructions.push(if zero_is_then {
                            Instruction::AndComplement { a: result, s: result, b: GENERAL_SCRATCH }
                        } else {
                            Instruction::And { a: result, s: result, b: GENERAL_SCRATCH }
                        });
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
            }
            if !function.locals.is_empty() {
                return Err(Diagnostic::error("locals combined with guards not yet supported"));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                // A logical (&&/||) condition short-circuits straight into the two return
                // blocks rather than computing the operator as a 0/1 value.
                if self.try_emit_short_circuit_guard(&guard.condition, &guard.value, return_expression, result)? {
                    return Ok(());
                }
                // `if (c) return X; return X` is degenerate: both paths return the same
                // value, and mwcc keeps the dead condition test then a single `blr`. Defer
                // rather than emit a spurious conditional return for the matching arms.
                if let (Expression::Variable(value_name), Expression::Variable(return_name)) = (&guard.value, return_expression) {
                    if value_name == return_name {
                        return Err(Diagnostic::error("a guard whose value equals the fall-through return is degenerate (roadmap)"));
                    }
                }
                // A guard condition that is a FLOAT comparison against a float CONSTANT (`if (a > 0.0f)
                // return 1; return 0;`) folds to the branchless `(a OP k) ? v : w` — the .text is
                // byte-exact — but mwcc allocates the if's (folded-away) branch labels BEFORE the pooled
                // float constant, so the constant's anonymous `@N` symbol number is offset by 2 from
                // ours. Modeling that counter is the low-value @N seam, so defer rather than emit a
                // mismatched `@N` symbol. (A non-guard `return a > 0.0f;` has no phantom labels and
                // matches; a two-variable float compare `a < b` pools no constant and is unaffected.)
                if matches!(&guard.condition, Expression::Binary { operator, left, right }
                    if crate::analysis::is_comparison(*operator)
                        && (matches!(left.as_ref(), Expression::FloatLiteral(_)) || matches!(right.as_ref(), Expression::FloatLiteral(_))))
                {
                    return Err(Diagnostic::error("a float-constant guard condition's pooled @N symbol is offset by mwcc's folded branch labels (roadmap)"));
                }
                // A null-guarded dereference (`if (!p) return CONST; return *p;` or the mirror
                // `if (p) return *p; return CONST;`) cannot fold branchless — dereferencing null is
                // unsafe — so mwcc branches on `p == 0` to the cold constant with the access in the
                // fall-through: `cmplwi p,0; beq COLD; <hot access>; blr; COLD: li CONST; blr`.
                if let Some((pointer, hot, cold)) = guarded_null_dereference(&guard.condition, &guard.value, return_expression, function.return_type) {
                    if let Some(pointer_register) = self.lookup_general(pointer) {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: pointer_register, immediate: 0 });
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 });
                        self.evaluate_tail(hot, function.return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        let cold_label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = cold_label;
                        }
                        self.evaluate_tail(cold, function.return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
                let select = guard_select(&guard.condition, &guard.value, return_expression);
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
        // A return value that is itself a call (`return h(p->a, p->b);`) emits its
        // argument setup here, after the body loop's hoist ran — so hoist again now.
        self.hoist_leading_arg_moves(lr_store_index);
        // A `float` function returning a double-precision value rounds to single
        // (`frsp`) before returning, as mwcc does.
        if function.return_type == Type::Float && self.is_double_value(return_expression) {
            self.output.instructions.push(Instruction::RoundToSingle { d: result, b: result });
        }
        self.emit_epilogue_and_return();
        Ok(())
    }

    /// mwcc fills the non-leaf prologue's `mflr`->LR-store latency with the leading
    /// run of register-ALU argument setup — parameter copies / derivations ready at
    /// entry: `stwu; mflr r0; mr r4,r3; mr r5,r3; stw r0,20(r1)`. A register move
    /// (`mr`/logical) or a register `addi` qualifies; an immediate load (`li`,
    /// `addi rD,0,imm`) and memory loads do not, and nothing touching r0 (which the
    /// LR store needs). The slot holds at most two, so the rest stay after the store.
    /// `lr_store_index` is the LR-store's position (only the general non-leaf path
    /// sets it; other paths pass `None` and this is a no-op).
    fn hoist_leading_arg_moves(&mut self, lr_store_index: Option<usize>) {
        let Some(store) = lr_store_index else { return };
        let mut run = 0;
        // A `li`-form argument (`addi rD,0,n`, `a == 0`) is hoisted by the saved-LR-store
        // scheduler when it leads — but once a register move (the indirect-call `mr
        // r12,fp`) has been hoisted ahead of the save, that scheduler can no longer find
        // the save at `mflr+1`, so the `li` must come along here. Allow it only after a
        // move, leaving the lone-`li` direct-call case to the other pass unchanged.
        let mut saw_move = false;
        // mwcc hoists at most the first TWO leading argument-setup instructions into the mflr->LR-store
        // gap (three moves, `sink3(a,b,c)`, keep the third after the store), so the run is capped at 2.
        while run < 2 {
            let Some(instruction) = self.output.instructions.get(store + 1 + run) else { break };
            let hoistable = match *instruction {
                Instruction::Or { a, s, b } => {
                    let movable = a != 0 && s != 0 && b != 0;
                    saw_move |= movable;
                    movable
                }
                Instruction::AddImmediate { d, a, .. } => d != 0 && (a != 0 || saw_move),
                // Any other single-cycle ALU arg-compute (`add`, `mullw`, `subf`, `and`, `xor`, shifts,
                // `neg`) leading a call's argument setup is hoisted the same way (`g(a+b)` ->
                // `add r3,r3,r4; stw r0`). A LOAD arg (`g(*p)`) is NOT hoisted — it stays after the LR
                // save — so this is an ALU whitelist; the no-r0-operand check keeps the hoisted compute
                // independent of the saved-LR store (which reads r0).
                ref other
                    if matches!(other,
                        Instruction::Add { .. } | Instruction::MultiplyLow { .. } | Instruction::SubtractFrom { .. }
                        | Instruction::And { .. } | Instruction::Xor { .. } | Instruction::ShiftLeftWord { .. }
                        | Instruction::ShiftRightWord { .. } | Instruction::ShiftRightAlgebraicWord { .. }
                        | Instruction::Negate { .. } | Instruction::ShiftLeftImmediate { .. }
                        | Instruction::ShiftRightAlgebraicImmediate { .. } | Instruction::ShiftRightLogicalImmediate { .. }
                        | Instruction::ClearLeftImmediate { .. } | Instruction::AndContiguousMask { .. }
                        | Instruction::RotateAndMask { .. } | Instruction::OrImmediate { .. }) =>
                {
                    let movable = mwcc_vreg::register_operands(other).iter().all(|operand| operand.register != 0);
                    saw_move |= movable;
                    movable
                }
                _ => false,
            };
            if !hoistable {
                break;
            }
            run += 1;
        }
        if run > 0 {
            self.output.instructions[store..=store + run].rotate_left(1);
        }
    }

    /// A leaf `void` body that is purely constant stores: mwcc materializes a
    /// repeated store value once and reuses the register (`li r0,0; stw; stw; stw`
    /// for struct/array zeroing). A run of *differing* constants instead needs the
    /// instruction scheduler (distinct registers, interleaved) — defer rather than
    /// emit the unscheduled form. Returns `false` (use the normal path) for bodies
    /// outside this shape, e.g. stores of register-resident values, which already
    /// match.
    /// `T y; if (c) y = A; else y = B; return y;` — both arms assign the same local,
    /// which is then returned, so the body is the select `return (c) ? A : B`. mwcc
    /// compiles it identically to `if (c) return A; return B`. A call in the body
    /// (value live across a branch) is the keystone's and defers.
    pub(crate) fn try_conditional_assign(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        // An initializer is DEAD here — both arms reassign the local before it is read (verified
        // below) and the handler builds the select purely from the arm values — so allow it:
        // `int b = INIT; if (c) b = A; else b = B; return b;` is the same select as the no-init form,
        // which mwcc compiles identically. (No-else keeps deferring to the initialized handler.)
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let returned = match &function.return_expression {
            Some(Expression::Variable(name)) => name,
            _ => return Ok(false),
        };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // Each arm must be exactly `y = <value>` for the returned local `y`.
        let arm_value = |body: &[Statement]| match body {
            [Statement::Assign { name, value }] if name == &local.name => Some(value.clone()),
            _ => None,
        };
        let (Some(when_true), Some(when_false)) = (arm_value(then_body), arm_value(else_body)) else {
            return Ok(false);
        };
        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        // `if (c) y = A; else y = B;` is the guard `if (c) y = A` with fall-through B
        // — mwcc normalizes a negated `if (!c)` the same way it does a guard return
        // (keep A as the in-place default, strip the `!`), so route through
        // guard_select rather than a bare `(c) ? A : B` select.
        let select = guard_select(condition, &when_true, &when_false);
        self.evaluate_tail(&select, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T y = INIT; if (c) y = NEW; return y;` (an `if` with no else) where INIT and NEW are
    /// constants. mwcc lowers this conditional ASSIGN as an early-return branch — distinct from the
    /// select/branchless idiom it uses for the equivalent guard `if(c) return NEW; return INIT;`:
    /// `<test c>; li result,INIT; b<!c>lr; li result,NEW; blr` (the false path returns the
    /// initializer already in the result; the true path falls through to the new value). Variable
    /// arms use a different move/staging form and are deferred here.
    pub(crate) fn try_conditional_assign_initialized(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        let Some(initializer) = &local.initializer else { return Ok(false) };
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name != &local.name {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let result = Eabi::general_result().number;
        let init_const = constant_value(initializer);
        let new_const = constant_value(value);

        // Resolve the variable arms' registers BEFORE emitting the compare (so a deferral leaves no
        // orphaned instructions). Each variable arm must be a leaf already in a register. The MOVE
        // form stages the initializer in a register (the scratch for a constant init, else the init
        // variable's own register); that staged register must differ from the result — mwcc uses a
        // different layout when the init variable already sits in the result — so defer that case.
        let new_register = match new_const {
            Some(_) => None,
            None => match leaf_name(value).and_then(|name| self.lookup_general(name)) {
                register @ Some(_) => register,
                None => return Ok(false),
            },
        };
        let stage = if init_const.is_some() && new_const.is_some() {
            None // both constant -> branch form, no staging register
        } else {
            let stage = match init_const {
                Some(_) => GENERAL_SCRATCH,
                None => match leaf_name(initializer).and_then(|name| self.lookup_general(name)) {
                    Some(register) => register,
                    None => return Ok(false),
                },
            };
            if stage == result {
                return Ok(false);
            }
            Some(stage)
        };

        // emit_condition_test returns the branch-if-FALSE options (a guard's forward-skip sense),
        // which is exactly the early-return / forward-skip-on-!c we want.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // Both arms constant: the early-return BRANCH form — return the initializer in place when
        // the condition does not hold, then fall through to the new value.
        let Some(stage) = stage else {
            self.load_integer_constant(result, init_const.unwrap());
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.load_integer_constant(result, new_const.unwrap());
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        };

        // A variable arm: the MOVE/staging form.
        if let Some(init_value) = init_const {
            self.load_integer_constant(stage, init_value);
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        match new_register {
            Some(register) => self.output.instructions.push(Instruction::move_register(stage, register)),
            None => self.load_integer_constant(stage, new_const.unwrap()),
        }
        let after = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = after;
        }
        self.output.instructions.push(Instruction::move_register(result, stage));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

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
                // 3+ distinct constants to small-data globals: mwcc materializes them into
                // r(N+1) descending to r3 and the last into the scratch r0, then stores all
                // in source order. Member/dereference targets reschedule with their base
                // register, and a duplicate constant shares one register — both defer.
                let all_globals = function.statements.iter().all(|statement| {
                    matches!(statement, Statement::Store { target: Expression::Variable(_), .. })
                });
                let count = constants.len();
                let mut distinct = constants.clone();
                distinct.sort_unstable();
                distinct.dedup();
                if !all_globals || distinct.len() != count || count + 1 > 12 {
                    return Err(Diagnostic::error("a run of 3+ differing constant stores needs the scheduler (roadmap)"));
                }
                let mut prematerialized = Vec::new();
                for (index, &constant) in constants.iter().enumerate() {
                    let register = if index + 1 < count { (count + 1 - index) as u8 } else { GENERAL_SCRATCH };
                    self.load_integer_constant(register, constant as i64);
                    prematerialized.push((constant, register));
                }
                self.prematerialized_constants = prematerialized;
                for statement in &function.statements {
                    self.emit_statement(statement)?;
                }
                self.prematerialized_constants.clear();
                self.emit_epilogue_and_return();
                return Ok(true);
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

    /// Two computed-value stores to distinct SDA globals (`gi = a+1; gj = b*2;`). mwcc
    /// overlaps the two value computations: it evaluates both first — the earlier into a
    /// real GPR, the later into the scratch `r0` — then stores both (`addi r3,r3,1; slwi
    /// r0,r4,1; stw r3; stw r0`), rather than the unscheduled `compute; store; compute;
    /// store`. We reproduce it by evaluating the first value into a fresh virtual (the
    /// allocator gives it the in-place GPR and keeps it off `r0`, since it is live across
    /// the second computation) and the second into `r0`, then both stores. Leaf/constant
    /// values (no computation to overlap) and 3+ stores fall through to their own paths.
    pub(crate) fn try_computed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() != 2
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // Both statements must store to a distinct SDA global. Each value is a single-op
        // computation or a constant; a bare register leaf needs no overlap and goes through
        // try_mixed_store_fill / the normal path.
        let mut stores = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            let Expression::Variable(name) = target else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            // Integer globals only — this path evaluates values through the general
            // (integer) evaluator; a float global/value goes through the float path.
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            // A single-instruction op over register operands, or a constant (materialized
            // with `li`, ordered as a low-latency value) — both shapes this path can
            // schedule. A memory read, comparison idiom, modulo, or nested value reorders
            // or needs more, and a bare register leaf goes through try_mixed_store_fill.
            if !self.is_single_op_register_value(value) && constant_value(value).is_none() {
                return Ok(false);
            }
            stores.push((name.clone(), pointee, value.clone()));
        }
        // At least one value must be a genuine computation. Two constants are the constant
        // fill's domain (it dedups a repeated value to one `li`); this overlap path would
        // emit a separate `li` per store.
        if !self.is_single_op_register_value(&stores[0].2) && !self.is_single_op_register_value(&stores[1].2) {
            return Ok(false);
        }
        if stores[0].0 == stores[1].0 {
            // Same target: the first store is dead (mwcc emits only the second) — a
            // dead-store elimination this path does not model, so defer.
            return Ok(false);
        }
        // The first store's value lives in a virtual (the allocator gives it the in-place
        // GPR, off r0 since it is live across the other op), the second in the scratch r0.
        // mwcc issues the heavier op first and stores the quicker value first, so order the
        // two evaluations and the two stores by latency.
        let high = [self.value_latency_is_high(&stores[0].2), self.value_latency_is_high(&stores[1].2)];
        // Evaluate the heavier value first so the allocator can reuse a spent operand
        // register for the lighter one. Weight: high-latency op > single-cycle op >
        // constant — a constant is just an `li`, materialized last once the computation has
        // freed its operand register (`gi=5; gj=a+1` → `addi r0,r3,1; li r3,5`, the `5`
        // reusing a's register).
        let weight = |is_high: bool, is_constant: bool| -> u8 {
            if is_high { 2 } else if is_constant { 0 } else { 1 }
        };
        let weights = [
            weight(high[0], constant_value(&stores[0].2).is_some()),
            weight(high[1], constant_value(&stores[1].2).is_some()),
        ];
        let first_register = self.fresh_virtual_general();
        if weights[1] > weights[0] {
            // The second value is the heavier op: compute it (into r0) first.
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
            self.evaluate_general(&stores[0].2, first_register)?;
        } else {
            self.evaluate_general(&stores[0].2, first_register)?;
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
        }
        if high[0] && !high[1] {
            // The first value is the long op: store the quicker second value first.
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH);
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register);
        } else {
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register);
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Two stores to distinct integer SDA globals where one value is a register-resident
    /// leaf parameter and the other a "filler" — a single-op computation (`gi=a+1; gj=b;`)
    /// or a constant (`gi=a; gj=5;`). mwcc produces the filler into the scratch, then stores
    /// the LEAF first (it is ready immediately) and the filler second — `addi r0,r3,1; stw
    /// r4,gj; stw r0,gi` or `li r0,5; stw r3,gi; stw r0,gj`. (Both-computed and computed+
    /// constant are the latency-ordered fill above; both-leaf is the normal path; both-
    /// constant is the constant fill.)
    pub(crate) fn try_mixed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() != 2
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        if stores[0].0 == stores[1].0 {
            return Ok(false);
        }
        // Exactly one value is a "filler" — a single-op computation or a constant — and the
        // other a register-resident leaf parameter (a global/memory leaf would need a load).
        // The filler is materialized into the scratch and the leaf stays in its register, so
        // both `gi=a+1; gj=b;` and `gi=a; gj=5;` reduce to: produce the filler, store the
        // leaf, store the filler.
        let filler = [
            self.is_single_op_register_value(&stores[0].2) || constant_value(&stores[0].2).is_some(),
            self.is_single_op_register_value(&stores[1].2) || constant_value(&stores[1].2).is_some(),
        ];
        let is_register_leaf = |value: &Expression| {
            matches!(value, Expression::Variable(name) if !self.globals.contains_key(name.as_str()))
        };
        let (filler, leaf) = if filler[0] && is_register_leaf(&stores[1].2) {
            (0usize, 1usize)
        } else if is_register_leaf(&stores[0].2) && filler[1] {
            (1usize, 0usize)
        } else {
            return Ok(false);
        };
        // The filler goes into the scratch; the leaf is already in its register, so store it
        // first, then the filler.
        let leaf_register = self.general_register_of_leaf(&stores[leaf].2)?;
        self.evaluate_general(&stores[filler].2, GENERAL_SCRATCH)?;
        self.emit_sda_global_store_from(&stores[leaf].0, stores[leaf].1, leaf_register);
        self.emit_sda_global_store_from(&stores[filler].0, stores[filler].1, GENERAL_SCRATCH);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Three or more stores to distinct integer SDA globals where exactly one value is a
    /// constant and the rest are register-resident leaf parameters (`gi=a; gj=b; gk=5;`).
    /// mwcc hoists the constant's `li` into the scratch up front and stores in source order
    /// — except a constant store cannot occupy the `li`'s one-cycle latency slot, so if the
    /// constant is the FIRST store it swaps with the next (leaf) store:
    ///
    ///     gi=a; gj=b; gk=5  ->  li r0,5; stw r3,gi; stw r4,gj; stw r0,gk   (source order)
    ///     gi=5; gj=a; gk=b  ->  li r0,5; stw r3,gj; stw r0,gi; stw r4,gk   (leading const swaps)
    ///
    /// (Two stores are the mixed fill; all-constant is the constant fill; a non-leaf, non-
    /// constant value among the rest needs the general scheduler and defers.)
    pub(crate) fn try_leaf_constant_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() < 3
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        // Distinct targets (a repeated target is a dead store this path does not model).
        for outer in 0..stores.len() {
            for inner in (outer + 1)..stores.len() {
                if stores[outer].0 == stores[inner].0 {
                    return Ok(false);
                }
            }
        }
        // Exactly one constant; every other value a register-resident leaf parameter.
        let mut constant_index = None;
        for (index, store) in stores.iter().enumerate() {
            if constant_value(&store.2).is_some() {
                if constant_index.is_some() {
                    return Ok(false);
                }
                constant_index = Some(index);
            } else if !matches!(&store.2, Expression::Variable(name) if !self.globals.contains_key(name.as_str())) {
                return Ok(false);
            }
        }
        let Some(constant_index) = constant_index else {
            return Ok(false);
        };
        let constant = constant_value(&stores[constant_index].2).unwrap();
        self.load_integer_constant(GENERAL_SCRATCH, constant as i64);
        // Source order, except a leading constant store swaps with the next store so it does
        // not sit in the `li`'s latency slot.
        let mut order: Vec<usize> = (0..stores.len()).collect();
        if constant_index == 0 {
            order.swap(0, 1);
        }
        for &index in &order {
            if index == constant_index {
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, GENERAL_SCRATCH);
            } else {
                let register = self.general_register_of_leaf(&stores[index].2)?;
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, register);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Whether `value` is a single-instruction arithmetic op over register-resident
    /// operands (parameters and constants) — the shape the computed-store-fill schedules.
    /// Includes the single-cycle ops (add/sub/and/or/xor/shift/neg/not, power-of-two
    /// multiply) and the multi-cycle but single-instruction ops (register/immediate
    /// multiply, divide), whose latency the fill orders around. Excluded (need more than a
    /// register-shuffle): modulo and comparisons (multi-instruction idioms), a nested
    /// value (needs an intermediate), and a memory read (needs load hoisting).
    pub(crate) fn is_single_op_register_value(&self, value: &Expression) -> bool {
        let is_register_leaf = |operand: &Expression| match operand {
            Expression::Variable(name) => !self.globals.contains_key(name.as_str()),
            Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
            _ => false,
        };
        match value {
            Expression::Binary { operator, left, right } => {
                is_register_leaf(left)
                    && is_register_leaf(right)
                    && matches!(
                        operator,
                        BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor
                            | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                            | BinaryOperator::Multiply | BinaryOperator::Divide
                    )
            }
            Expression::Unary { operator: UnaryOperator::Negate | UnaryOperator::BitNot, operand } => is_register_leaf(operand),
            _ => false,
        }
    }

    /// Whether a single-op value is multi-cycle (a register or non-power-of-two multiply,
    /// or a divide) rather than single-cycle. mwcc issues the long op early and stores the
    /// quick value first; the computed-store-fill orders the two values by this.
    fn value_latency_is_high(&self, value: &Expression) -> bool {
        let is_power_of_two = |operand: &Expression| {
            matches!(operand, Expression::IntegerLiteral(n) if *n > 0 && (*n & (*n - 1)) == 0)
        };
        match value {
            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
                !(is_power_of_two(left) || is_power_of_two(right))
            }
            Expression::Binary { operator: BinaryOperator::Divide, .. } => true,
            _ => false,
        }
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
            // A small-data (SDA21) integer global store folds the relocation into the
            // store itself (`stw r0, g@sda21`) — no base register, and it never writes the
            // scratch — so a constant fill can keep its value live across it. An absolute-
            // addressing global needs a base register, so it stays excluded.
            Expression::Variable(name) => {
                matches!(self.behavior.global_addressing, GlobalAddressing::SmallData)
                    && self.globals.get(name.as_str()).is_some_and(|global_type| !matches!(global_type, Type::Float | Type::Double))
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
    /// A `void` function whose whole body is `do { …calls… } while (--counter);`
    /// with the counter a parameter: mwcc keeps the counter in a callee-saved
    /// register (r31), runs the body, then `addic. r31,r31,-1` (decrement, set CR0)
    /// and `bne` back to the loop top. Returns whether this path applied.
    fn try_do_while_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let kind = *kind;
        if !matches!(kind, LoopKind::DoWhile | LoopKind::While) {
            return Ok(false);
        }
        // The condition must be `--counter` (a parameter decrement), which the
        // parser lowered to `counter = counter - 1`.
        let counter = match condition {
            Expression::Assign { target, value } => match (target.as_ref(), value.as_ref()) {
                (
                    Expression::Variable(name),
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right },
                ) if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1)) =>
                {
                    name.clone()
                }
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| parameter.name == counter) {
            return Ok(false);
        }
        // The body must be straight-line calls that do not pass the counter as an
        // argument (the first such use would stay in the incoming register — the
        // value-location nuance the callee-saved path also defers).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
            return Ok(false);
        }
        if body.iter().any(|statement| matches!(statement, Statement::Expression(e) if expression_reads_name(e, &counter))) {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let (class, incoming) = match self.locations.get(&counter) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }

        // Prologue: save the link register and r31, move the counter into r31.
        const SAVED: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![SAVED];
        // The loop's internal labels advance mwcc's anonymous-`@N` counter — by 6
        // for a do-while, by 4 for a while (the extra top branch, fewer labels).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: SAVED, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Or { a: SAVED, s: incoming, b: incoming });
        if let Some(location) = self.locations.get_mut(&counter) {
            location.register = SAVED;
        }

        // A while loop tests the condition first: branch down to the
        // decrement-and-test, which falls through into the body on re-entry.
        let skip_to_condition = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        // The loop body, then the decrement-and-test and the backward branch.
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        if let Some(index) = skip_to_condition {
            let condition = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition;
            }
        }
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: SAVED, a: SAVED, immediate: -1 });
        // `bne body_top`: branch when CR0[EQ] is clear (BO=4, BI=2). Backward, which
        // the encoder resolves from the instruction indices.
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: body_top });

        // Epilogue, emitted in final order (the loop's branch makes the scheduler and
        // the LR-reload hoist bail): the LR reload comes before the r31 reload.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        self.output.instructions.push(Instruction::LoadWord { d: SAVED, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A leaf `void` function whose body is a single non-counting `while` loop (truthy condition)
    /// whose body is pure in-place increments/decrements of register parameters (`while (*p) p++;`).
    /// mwcc does
    /// NOT unroll these; it emits the rotated form `[b COND;] BODY: <addi>; COND: <test>; <bne BODY>;
    /// blr` with no frame (leaf). The body is emitted directly via `evaluate_general` into each
    /// variable's OWN register, so the loop-carried mutation stays in place rather than being
    /// value-tracked across the back-edge (the linear value tracker has no back-edge). A store in the
    /// body (mwcc hoists loop-invariant store values — not modeled), an empty body (different
    /// structure: the condition is the loop top), a call, a local, a guard, a counted loop (mwcc
    /// unrolls), or a non-void return falls through to defer.
    fn try_emit_increment_while(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let kind = *kind;
        // A do-while (mwcc fuses the body increment with the next condition load into `lwzu`) or a
        // comparison condition (`while (p < e)` — mwcc computes the trip count and emits a counted
        // CTR loop, `mtctr`/`bdnz`) is deferred; only a `while` with a truthy condition keeps the
        // plain rotated form this models.
        if !matches!(kind, LoopKind::While) || body.is_empty() {
            return Ok(false);
        }
        // A comparison of the loop counter against a loop-invariant bound (`while (p < e)`) lets mwcc
        // compute the trip count and emit a counted CTR loop, so it is deferred. But a comparison of a
        // DATA-DEPENDENT value (`while (*p != 0)`, `while (*p > 0)`) has no computable trip count and
        // stays the rotated form — allow it when one side is a dereference and the other a constant.
        if let Expression::Binary { operator, left, right } = condition {
            if crate::analysis::is_comparison(*operator) {
                let dereference_vs_constant = (matches!(left.as_ref(), Expression::Dereference { .. }) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
                    || (matches!(right.as_ref(), Expression::Dereference { .. }) && matches!(left.as_ref(), Expression::IntegerLiteral(_)));
                if !dereference_vs_constant {
                    return Ok(false);
                }
            }
        }
        // Every body statement must be an in-place `var = var +/- const` on a register parameter — the
        // increment/decrement of a scan pointer or index. No stores, calls, loads, or nested control.
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            if self.lookup_general(name).is_none() {
                return Ok(false);
            }
            // The incremented variable must be a POINTER: mwcc countifies an integer increment loop
            // (`while (x) x++;` -> neg/mtctr/bdnz, trip count `-x`) but leaves a pointer scan as the
            // rotated form this models.
            if self.locations.get(name).map_or(true, |location| location.pointee.is_none()) {
                return Ok(false);
            }
            let is_increment = matches!(value, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(_)));
            if !is_increment {
                return Ok(false);
            }
        }
        // The loop's labels advance mwcc's anonymous-`@N` counter (4 for a while, 6 for a do-while).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        // A while tests the condition first: branch down to the test; a do-while falls into the body.
        let skip = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        let body_top = self.output.instructions.len();
        for statement in body {
            if let Statement::Assign { name, value } = statement {
                let register = self.lookup_general(name).expect("gated to a register variable above");
                self.evaluate_general(value, register)?;
            }
        }
        if let Some(index) = skip {
            let condition_at = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition_at;
            }
        }
        // emit_condition_test gives the body-SKIP branch (taken when the condition is FALSE); the loop
        // branches BACK to the body when it is TRUE, so invert the BO (branch-if-clear <-> -if-set).
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output.instructions.push(Instruction::BranchConditionalForward { options: back, condition_bit, target: body_top });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A `void` function whose body is a counting `for (i = 0; i < bound; i++)`
    /// loop with a parameter bound: mwcc puts the counter in r31 (callee-saved,
    /// initialised to 0) and the bound in r30, branches to the test, and runs
    /// `BODY: <body>; addi r31,r31,1; cmpw r31,r30; blt BODY`. The body may use the
    /// counter (passed as a call argument). Returns whether this path applied.
    fn try_for_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !self.frame_slots.is_empty() || function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(init), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = 0`.
        let counter = match init {
            Expression::Assign { target, value } if matches!(value.as_ref(), Expression::IntegerLiteral(0)) => {
                match target.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i < bound`.
        let bound = match condition {
            Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter) =>
            {
                match right.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i = i + 1`.
        let step_increments = matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if *name == counter)
            && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1))));
        if !step_increments {
            return Ok(false);
        }
        // The counter is the function's only local (uninitialised — the for-clause
        // sets it); the bound is a parameter.
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.name != counter || local.initializer.is_some() {
            return Ok(false);
        }
        if !function.parameters.iter().any(|parameter| parameter.name == bound) {
            return Ok(false);
        }
        // The body must be straight-line calls referencing no register value other
        // than the counter (the bound, and any other parameter, would each need
        // their own callee-saved register — deferred).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
            return Ok(false);
        }
        let reads_other_parameter = body.iter().any(|statement| match statement {
            Statement::Expression(expression) => function
                .parameters
                .iter()
                .any(|parameter| parameter.name != counter && expression_reads_name(expression, &parameter.name)),
            _ => false,
        });
        if reads_other_parameter {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let bound_incoming = match self.locations.get(&bound) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };

        // Prologue: r31 = counter (init 0), r30 = bound, saved at the top of a frame.
        const COUNTER: u8 = 31;
        const BOUND: u8 = 30;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![COUNTER, BOUND];
        self.output.anonymous_label_bump = 5;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::Or { a: BOUND, s: bound_incoming, b: bound_incoming });
        let signed = self.signed_of(local.declared_type);
        self.locations.insert(
            counter.clone(),
            Location { class: ValueClass::General, register: COUNTER, signed, width: 32, pointee: None, stride: None },
        );
        if let Some(location) = self.locations.get_mut(&bound) {
            location.register = BOUND;
        }

        // Branch to the test; the body falls into the step, then the compare loops.
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: COUNTER, immediate: 1 });
        let condition_index = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_index;
        }
        self.output.instructions.push(Instruction::CompareWord { a: COUNTER, b: BOUND });
        // `blt body_top` (BO=12 branch-if-true, BI=0 the LT bit).
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: body_top });

        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A straight-line non-leaf function whose parameters live across its call(s):
    /// mwcc copies each into a callee-saved register at entry (saved/reloaded around
    /// the frame) so it survives the calls. The registers are assigned by parameter
    /// order — the LAST live parameter gets r31, the next r30, and so on — and the
    /// body/return then read the values from those registers. Returns whether it
    /// applied. (Locals, floats, and values passed to a call still defer.)
    fn try_callee_saved(&mut self, function: &Function) -> Compilation<bool> {
        // Address-taken locals are handled by the frame-resident path before this.
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        // The body is straight-line calls and stores (control flow routes through its own
        // paths). A trailing store sink (`foo(); gi = a;`) saves the live value, runs the
        // calls, then stores it from the callee-saved register; mwcc orders that epilogue's
        // LR reload before the GPR reload (epilogue_lr_first), unlike the return sink.
        if function.statements.iter().any(|statement| !matches!(statement, Statement::Expression(_) | Statement::Store { .. })) {
            return Ok(false);
        }
        let has_store = function.statements.iter().any(|statement| matches!(statement, Statement::Store { .. }));
        if matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        let Some(live) = values_live_across_call(function) else {
            return Ok(false);
        };
        if live.is_empty() {
            return Ok(false);
        }
        // Every live value must be a general-class parameter (locals defer), and none
        // may be passed to a call — the first such argument use stays in the incoming
        // register (mwcc skips the move until a call clobbers it), which needs
        // value-location tracking not modeled here.
        let passed_to_call = function.statements.iter().any(|statement| match statement {
            Statement::Expression(expression) => live.iter().any(|name| expression_reads_name(expression, name)),
            _ => false,
        });
        if passed_to_call {
            return Ok(false);
        }
        // (parameter index, name, incoming register) for each promoted value.
        let mut promoted: Vec<(usize, String, u8)> = Vec::new();
        for name in &live {
            let Some(index) = function.parameters.iter().position(|parameter| &parameter.name == name) else {
                return Ok(false);
            };
            let (class, incoming) = match self.locations.get(name) {
                Some(location) => (location.class, location.register),
                None => return Ok(false),
            };
            if class != ValueClass::General {
                return Ok(false);
            }
            promoted.push((index, name.clone(), incoming));
        }
        // Highest register (r31) to the last parameter, descending toward the first.
        promoted.sort_by_key(|(index, _, _)| *index);

        let count = promoted.len();
        // A store sink takes one or two saved values: the epilogue reloads all-but-the-
        // lowest GPR, then LR, then the lowest, matching mwcc for count 1 and 2 (three or
        // more reschedule the LR reload by register death — `lwz r31; lwz r30; lwz r29; lwz
        // r0` — and defer). A second saved value must be void; a value returned alongside
        // two saved values interleaves the return move with the epilogue and is not modeled.
        if has_store && (count > 2 || (count == 2 && function.return_type != Type::Void)) {
            return Ok(false);
        }
        // A two-value store sink stores the saved values directly (`gi = a; gj = b;`). A
        // computed store (`gi = a + 1;`) reschedules around the two saves/epilogue and is
        // deferred; the single-value sink still allows a computed store.
        if has_store
            && count == 2
            && !function.statements.iter().all(|statement| match statement {
                Statement::Store { value, .. } => matches!(value, Expression::Variable(_)),
                _ => true,
            })
        {
            return Ok(false);
        }
        // With more than one saved value RETURNED, mwcc's scheduler interleaves the
        // epilogue restores with the post-call computation by register death — which we
        // don't model yet. It coincides with "all restores after" only when the values
        // combine in a single low-latency instruction (`a+b`, `a-b`, `a&b`); two
        // values through a multiply, or three or more values (multi-step), reschedule
        // the restores. Restrict count > 1 to that one safe shape. (A two-value store sink
        // has its own epilogue order above, so it skips this return-shape gate.)
        if count >= 2 && !has_store {
            let single_op = matches!(
                function.return_expression.as_ref(),
                Some(Expression::Binary { operator, left, right })
                    if count == 2
                        && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                        && matches!(left.as_ref(), Expression::Variable(_))
                        && matches!(right.as_ref(), Expression::Variable(_))
            );
            if !single_op {
                return Ok(false);
            }
        }
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        self.callee_saved = (0..count as u8).map(|rank| 31 - rank).collect();
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: frame_size + 4 });
        // A store sink reloads the saved LR before the GPR reloads in the epilogue.
        self.epilogue_lr_first = has_store;
        // Save and move each, highest register first (r31 ← last parameter), with the
        // save interleaved before its move, as mwcc emits them.
        for (rank, (_, name, incoming)) in promoted.iter().rev().enumerate() {
            let register = 31 - rank as u8;
            let offset = frame_size - 4 * (rank as i16 + 1);
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset });
            self.output.instructions.push(Instruction::Or { a: register, s: *incoming, b: *incoming });
            if let Some(location) = self.locations.get_mut(name) {
                location.register = register;
            }
        }

        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        if function.return_type != Type::Void {
            let result = Eabi::general_result().number;
            let return_expression = function
                .return_expression
                .as_ref()
                .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A void function whose body is two or more calls that each pass the SAME argument
    /// list — all the parameters, in order — `f(a,b){ g(a,b); h(a,b); }` (the single-
    /// parameter `f(x){ g(x); h(x); }` is the common case). Each parameter is live across
    /// the calls, so mwcc saves them in callee-saved registers up front (r31 to the last
    /// parameter, descending), interleaving each save with its move; the first call uses
    /// the incoming argument registers directly (no moves), and each later call restores
    /// them. One of the most common real shapes (a state handed to several functions).
    fn try_callee_saved_call_args(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.parameters.is_empty() || matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        // A non-void function returns one of the parameters (or a register-only expression
        // over them) after the calls — the return is the post-call use that keeps them
        // live; a void function needs two or more calls for that liveness. A call in the
        // return is a different shape (call-result), so it defers.
        let returns_value = function.return_type != Type::Void;
        if returns_value {
            match &function.return_expression {
                Some(expression) if !expression_has_call(expression) => {}
                _ => return Ok(false),
            }
        } else if function.statements.len() < 2 {
            return Ok(false);
        }
        // Every statement must be a call whose arguments are exactly the parameters in
        // order, so the first call needs no moves and the live set is all the parameters.
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else { return Ok(false) };
            if arguments.len() != function.parameters.len() {
                return Ok(false);
            }
            for (argument, parameter) in arguments.iter().zip(&function.parameters) {
                if !matches!(argument, Expression::Variable(name) if name == &parameter.name) {
                    return Ok(false);
                }
            }
        }
        // Each parameter's incoming register; all must be general-class.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        let count = incoming.len();
        let frame_size = ((8 + 4 * count as i32 + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        self.callee_saved = (0..count as u8).map(|rank| 31 - rank).collect();
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: frame_size + 4 });
        // Save each parameter to a callee-saved register — highest (r31) to the last
        // parameter, descending — interleaving the store with the move, as mwcc emits.
        for (rank, (_, incoming_register)) in incoming.iter().rev().enumerate() {
            let register = 31 - rank as u8;
            let offset = frame_size - 4 * (rank as i16 + 1);
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset });
            self.output.instructions.push(Instruction::Or { a: register, s: *incoming_register, b: *incoming_register });
        }
        // The first call finds the parameters still in their incoming registers (no
        // moves); afterward they live only in their callee-saved registers.
        self.emit_statement(&function.statements[0])?;
        for (rank, (name, _)) in incoming.iter().rev().enumerate() {
            let register = 31 - rank as u8;
            if let Some(location) = self.locations.get_mut(name) {
                location.register = register;
            }
        }
        for statement in &function.statements[1..] {
            self.emit_statement(statement)?;
        }
        // A non-void return reads the parameters from their callee-saved registers; the
        // epilogue scheduler hoists the LR reload ahead of this move, matching mwcc.
        if returns_value {
            let result = Eabi::general_result().number;
            let return_expression = function.return_expression.as_ref().unwrap();
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f(...) + x;` — a single general parameter `x` kept live across a call that sits INSIDE
    /// the return expression, then combined with the call's result. mwcc saves `x` in r31 before the
    /// call (`mr r31,r3`), runs the call (whose argument, when it is `x`, is already in the incoming
    /// register, so no move precedes it), reloads LR, then combines from the callee-saved register
    /// (`add r3,r31,r3` — the saved value first). The call is argument-free or forwards exactly the
    /// parameter; a computed/constant argument schedules its materialization differently and defers.
    /// Only `+` for now (a commutative low-latency op whose `OP r3,r31,r3` order holds either source
    /// side); other ops and multi-parameter shapes are follow-ups.
    fn try_callee_saved_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 {
            return Ok(false);
        }
        let param = &function.parameters[0];
        let (class, param_register) = match self.locations.get(&param.name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        let Some(Expression::Binary { operator: BinaryOperator::Add, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let is_param = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == &param.name);
        let (call_name, call_arguments) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, other) if is_param(other) => (name, arguments),
            (other, Expression::Call { name, arguments }) if is_param(other) => (name, arguments),
            _ => return Ok(false),
        };
        // The call takes no arguments or forwards exactly the parameter (already in its incoming
        // register); anything else materializes an argument on a different schedule.
        if !(call_arguments.is_empty() || (call_arguments.len() == 1 && is_param(&call_arguments[0]))) {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and r31.
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![31];
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        // Save the live parameter into r31 before the call clobbers its incoming register.
        self.output.instructions.push(Instruction::Or { a: 31, s: param_register, b: param_register });
        self.emit_call(call_name, call_arguments, None, false)?;
        // Combine the saved parameter (r31) with the call result (r3) — the saved value first.
        let result = Eabi::general_result().number;
        self.output.instructions.push(Instruction::Add { d: result, a: 31, b: result });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// One or two locals that are CALL RESULTS, live across later calls, then returned:
    /// `int z = g(); h(); return z;` or `int a = g1(); int b = g2(); h(); return a+b;`.
    /// mwcc preserves them in r31 (and r30) across the later calls — each producing call
    /// is followed by a move into its callee-saved register, all saved up front. The
    /// single-local return may post-process z (`z + 1`); the two-local return must be a
    /// single low-latency op of both (`a + b`), as in [`Self::try_callee_saved`].
    /// (Parameters live across calls go through that path.) Narrowly shaped.
    fn try_callee_saved_call_result(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // One or two general-int locals, each initialized by an argument-free call.
        let count = function.locals.len();
        if count == 0 || count > 2 {
            return Ok(false);
        }
        let mut init_calls: Vec<(String, Vec<Expression>)> = Vec::new();
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
                return Ok(false);
            }
            let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                return Ok(false);
            };
            init_calls.push((name.clone(), arguments.clone()));
        }
        // A producing call's arguments are allowed only in the single-local case, and
        // only when they forward parameters in their natural register positions (arg i
        // is parameter i, all parameters general) — then the parameters are already in
        // place, no moves are emitted, and the sequence matches the argument-free shape.
        // A constant/reordered argument would schedule its materialization differently,
        // and a second producing call's parameter would be call-clobbered; both defer.
        let params_all_general = !function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double));
        for (index, (_, arguments)) in init_calls.iter().enumerate() {
            if arguments.is_empty() {
                continue;
            }
            let forwards_parameters = count == 1
                && index == 0
                && params_all_general
                && arguments.len() <= function.parameters.len()
                && arguments
                    .iter()
                    .enumerate()
                    .all(|(position, argument)| matches!(argument, Expression::Variable(name) if name == &function.parameters[position].name));
            if !forwards_parameters {
                return Ok(false);
            }
        }
        // The return reads no parameter (it would be a call-clobbered register) and no
        // global (its load reschedules against the epilogue). A single local may be
        // post-processed (`z + 1`); two locals must combine in one low-latency op
        // (`a + b`), the only shape whose restores aren't rescheduled.
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if function.parameters.iter().any(|parameter| expression_reads_name(return_expr, &parameter.name)) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(return_expr, name)) {
            return Ok(false);
        }
        if count == 1 {
            if !expression_reads_name(return_expr, &function.locals[0].name) {
                return Ok(false);
            }
        } else {
            let single_op = matches!(return_expr, Expression::Binary { operator, left, right }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                    && matches!(left.as_ref(), Expression::Variable(_))
                    && matches!(right.as_ref(), Expression::Variable(_)));
            if !single_op || !function.locals.iter().all(|local| expression_reads_name(return_expr, &local.name)) {
                return Ok(false);
            }
        }
        // The body is one or more straight-line argument-free calls (so the locals are
        // genuinely live across a call).
        if function.statements.is_empty() {
            return Ok(false);
        }
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if !arguments.is_empty() {
                return Ok(false);
            }
        }

        // Prologue: a frame saving the link register and the callee-saved registers
        // (r31, then r30), all up front, highest at the top of the frame.
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        self.callee_saved = (0..count as u8).map(|rank| 31 - rank).collect();
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -frame_size });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: frame_size + 4 });
        for rank in 0..count {
            let register = 31 - rank as u8;
            let offset = frame_size - 4 * (rank as i16 + 1);
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset });
        }

        // Each local: its producing call, then move r3 into the local's callee-saved
        // register — the first local takes the lowest (r30 when there are two), the last
        // takes r31, matching mwcc's `bl g1; mr r30,r3; bl g2; mr r31,r3`.
        for (index, local) in function.locals.iter().enumerate() {
            let (init_name, init_arguments) = &init_calls[index];
            self.emit_call(init_name, init_arguments, None, false)?;
            let register = (32 - count + index) as u8;
            self.output.instructions.push(Instruction::Or { a: register, s: 3, b: 3 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register, signed, width: 32, pointee: None, stride: None },
            );
        }

        // The later calls, then the return. The LR-reload hoist places the saved-LR
        // reload right after the last call, matching mwcc's epilogue order.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local COMPUTED from parameters (no call in its initializer) that is live
    /// across a call — passed to it and/or post-processed in the return:
    /// `int z = x + 1; g(z); return z;`. z is computed into r31 before the call, used
    /// from r31 (as a call argument and/or the return), then reloaded. Argument calls may
    /// pass only z and constants (a parameter would be call-clobbered).
    fn try_callee_saved_computed_local(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.locals.len() != 1 {
            return Ok(false);
        }
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(initializer) = local.initializer.as_ref() else {
            return Ok(false);
        };
        // A genuinely computed initializer: not a bare variable (that keeps its source
        // register), not a call (the call-result path), reading no global.
        if matches!(initializer, Expression::Variable(_)) || expression_has_call(initializer) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(initializer, name)) {
            return Ok(false);
        }
        // One or more argument calls whose arguments read only z (preserved in r31) and
        // constants; the return reads z and no parameter/global. (A parameter in either
        // would be read from a call-clobbered register.)
        if function.statements.is_empty() {
            return Ok(false);
        }
        let reads_param_or_global = |this: &Self, expression: &Expression| {
            function.parameters.iter().any(|parameter| expression_reads_name(expression, &parameter.name))
                || this.globals.keys().any(|name| expression_reads_name(expression, name))
        };
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if arguments.iter().any(|argument| reads_param_or_global(self, argument)) {
                return Ok(false);
            }
        }
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !expression_reads_name(return_expr, &local.name) || reads_param_or_global(self, return_expr) {
            return Ok(false);
        }

        // Prologue, then compute z into r31, then the argument calls, then the return.
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![31];
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 12 });
        self.evaluate_general(initializer, 31)?;
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: 31, signed, width: 32, pointee: None, stride: None },
        );
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn emit_epilogue_and_return(&mut self) {
        let reload_saved_gprs = |generator: &mut Self| {
            for (index, &register) in generator.callee_saved.iter().enumerate() {
                let offset = generator.frame_size - 4 * (index as i16 + 1);
                generator.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
        };
        if self.epilogue_lr_first && self.non_leaf {
            // Store-sink callee-saved: mwcc reloads all saved GPRs except the LOWEST, then
            // the saved LR, then the lowest GPR (count==1: `lwz r0; lwz r31`; count==2: `lwz
            // r31; lwz r0; lwz r30`). A register-death schedule this reproduces for one or
            // two saved values; three or more reschedule it (the sink restricts to <= 2).
            let last = self.callee_saved.len().saturating_sub(1);
            for (index, &register) in self.callee_saved.iter().enumerate() {
                if index == last {
                    continue;
                }
                let offset = self.frame_size - 4 * (index as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            if let Some(&register) = self.callee_saved.last() {
                let offset = self.frame_size - 4 * (last as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        } else {
            // Reload callee-saved registers (highest first, from the top of the frame)
            // before the saved-LR reload, so that reload stays directly before `mtlr`
            // where the hoist pass finds it and issues it right after the last call.
            reload_saved_gprs(self);
            if self.non_leaf {
                self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
                self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            }
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
            // Loops (while/do-while/for) parse but defer until the loop codegen
            // (backward branch + the callee-saved counter) lands.
            Statement::Loop { .. } => Err(Diagnostic::error("loop codegen is not implemented yet (roadmap)")),
        }
    }

    /// A trailing leaf `if (c) then; [else otherwise | else if …]` in a void
    /// function. With no else, the false path is a conditional return (the body
    /// then falls through to the function `blr`). With an else, branch over the
    /// then-body (and its `blr`) to the else, which is either a single statement
    /// or a nested trailing if (an `else if` chain). Each then-body is a single
    /// statement — multiple statements need the scheduler.
    fn emit_trailing_if(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement]) -> Compilation<()> {
        // `if (cond) tgt = c1; else tgt = c2;` — both arms a single constant store to the
        // same target — is one store of a select. mwcc branchless-ifies consecutive
        // constants (`srawi; addi`) and branch-materializes others into one register, then
        // stores once; route it through the conditional store path (byte-exact-or-defer)
        // rather than the two-branch form.
        if let ([Statement::Store { target: then_target, value: then_value }],
                [Statement::Store { target: else_target, value: else_value }]) = (then_body, else_body)
        {
            if same_operand(then_target, else_target)
                && constant_value(then_value).is_some()
                && constant_value(else_value).is_some()
            {
                let select = Expression::Conditional {
                    condition: Box::new(condition.clone()),
                    when_true: Box::new(then_value.clone()),
                    when_false: Box::new(else_value.clone()),
                };
                return self.emit_store(then_target, &select);
            }
        }
        if then_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement if-body needs the scheduler (roadmap)"));
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if else_body.is_empty() {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            return self.emit_statement(&then_body[0]);
        }
        // An `else if` chain keeps the two-exit form: the then-arm returns (`blr`), then
        // the nested trailing `if`.
        if let [Statement::If { condition: else_condition, then_body: else_then, else_body: else_else }] = else_body {
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.emit_statement(&then_body[0])?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
            return self.emit_trailing_if(else_condition, else_then, else_else);
        }
        if else_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement else-body needs the scheduler (roadmap)"));
        }
        // For a truthy condition (a bare register compare) with global-store arms, mwcc
        // uses the re-test idiom: the then-arm falls through to a *re-test* of the
        // condition and a conditional return, then the else — `cmpwi; beq L; A; L: cmpwi;
        // bnelr; B; blr`. A comparison condition re-tests by branchless recomputation (not
        // a second compare), and member/base-register arms keep the two-exit form; both of
        // those route to the two-exit branch below.
        let truthy = matches!(condition, Expression::Variable(_))
            || matches!(condition, Expression::Unary { operator: UnaryOperator::LogicalNot, operand } if matches!(operand.as_ref(), Expression::Variable(_)));
        let is_global_store = |statement: &Statement| {
            matches!(statement, Statement::Store { target: Expression::Variable(name), .. } if self.globals.contains_key(name.as_str()))
        };
        let use_retest = truthy && is_global_store(&then_body[0]) && is_global_store(&else_body[0]);
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.emit_statement(&then_body[0])?;
        if use_retest {
            let label = self.output.instructions.len();
            let (retest_options, retest_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: retest_options ^ 8, condition_bit: retest_bit });
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        } else {
            // Two-exit form: the then-arm returns, the conditional branch lands on the else.
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        }
        self.emit_statement(&else_body[0])?;
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

    /// A leaf `if (c) { … return [v]; }` whose then-body ends in an early return:
    /// forward-branch over the body when the condition is false, emit the body
    /// (the `return` materializes the value and runs the epilogue — `blr` for a
    /// leaf), then patch the branch to land on the continuation (the rest of the
    /// function, which supplies the other exit).
    fn emit_if_early_return(&mut self, condition: &Expression, then_body: &[Statement], return_type: Type) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for statement in then_body {
            if let Statement::Return(value) = statement {
                if let Some(value) = value {
                    let result = match return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate_tail(value, return_type, result)?;
                }
                self.emit_epilogue_and_return();
            } else {
                self.emit_statement(statement)?;
            }
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        Ok(())
    }

    /// A non-leaf function whose body begins with `if (c) { …calls…; return X; }`
    /// (the if is the first statement) followed by a straight-line continuation
    /// that supplies the other return. mwcc schedules the condition test into the
    /// prologue (between `mflr` and the LR store), the early return materializes X
    /// and branches to a SHARED epilogue, and the continuation falls into that same
    /// epilogue. Returns whether this path took over the whole body.
    fn try_non_leaf_if_first_early_return(&mut self, function: &Function) -> Compilation<bool> {
        // Shape: `if (c) { body…; return; } continuation…`, the if first, non-leaf,
        // no guards/locals, no else. The general/void return type only (a float
        // early return adds the FP result register — deferred).
        let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !function_makes_call(function)
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !else_body.is_empty()
            || matches!(function.return_type, Type::Float | Type::Double)
        {
            return Ok(false);
        }
        // The then-body must be straight-line calls/stores ending in the early
        // return; the continuation must likewise be straight-line (no nested
        // control flow, which would need its own branches).
        let Some((early_return, leading)) = then_body.split_last() else {
            return Ok(false);
        };
        let early_value = match early_return {
            Statement::Return(value) => value,
            _ => return Ok(false),
        };
        // Only calls may sit in the then-body or continuation: a call is a
        // scheduling barrier, so the value materialization that follows stays put.
        // A store would let mwcc's scheduler interleave the value into the store
        // sequence (`li r0,5; li r3,2; stw` rather than `li r0,5; stw; li r3,2`),
        // which this straight-line emission cannot reproduce — defer those.
        let call_only = |statement: &Statement| matches!(statement, Statement::Expression(_));
        if !leading.iter().all(call_only) || !rest.iter().all(call_only) {
            return Ok(false);
        }
        // A void function ends after its statements; a value-returning one supplies
        // the other exit through the trailing `return` expression. The early
        // return's value-ness must match (both void or both a value).
        let returns_value = function.return_type != Type::Void;
        if returns_value != early_value.is_some() || returns_value != function.return_expression.is_some() {
            return Ok(false);
        }
        // The condition test must be schedulable into the prologue: it cannot itself
        // make a call (that would need its own frame-aware sequencing).
        if expression_has_call(condition) {
            return Ok(false);
        }
        // A value computed AFTER a call on its path cannot be read from a
        // caller-saved register (the call clobbers it); mwcc would spill the source
        // to a callee-saved register (r31) and restructure the whole frame — that
        // is the next subsystem and is deferred. So a return value that follows a
        // call on its own path must be a compile-time constant (no register read).
        // The early return follows the then-body's calls; the continuation value
        // follows the continuation's calls (the false path skipped the then-body).
        let then_calls = leading.iter().any(statement_has_call);
        let rest_calls = rest.iter().any(statement_has_call);
        if then_calls && early_value.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }
        if rest_calls && function.return_expression.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        self.non_leaf = true;
        self.frame_size = 16;
        // The if's branch labels advance mwcc's anonymous-`@N` counter by 2.
        self.output.anonymous_label_bump = 2;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        // False path skips the then-body to the continuation.
        let continuation_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        // The then-body: the leading calls/stores, then the early return's value.
        for statement in leading {
            self.emit_statement(statement)?;
        }
        if let Some(value) = early_value {
            self.evaluate_tail(value, function.return_type, result)?;
        }
        // The early return branches to the shared epilogue. Reserve the slot — if
        // the continuation turns out to emit nothing (e.g. `return a` with `a`
        // already in the result register), mwcc lets the early return fall through
        // to the epilogue rather than branch, so the slot is dropped below.
        let branch_slot = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let continuation_label = self.output.instructions.len();
        for statement in rest {
            self.emit_statement(statement)?;
        }
        if let Some(return_expression) = &function.return_expression {
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        if self.output.instructions.len() == continuation_label {
            // The continuation produced no instructions: the early return falls
            // straight through to the epilogue, and the false path targets the
            // epilogue directly. Drop the unnecessary branch.
            self.output.instructions.remove(branch_slot);
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
                *target = epilogue_label;
            }
        } else {
            // A non-empty continuation: the false path lands on it, and the early
            // return branches over it to the shared epilogue.
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
                *target = continuation_label;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[branch_slot] {
                *target = epilogue_label;
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
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

        // mwcc reuses one `cmpwi` across consecutive guards that test the same operand against the
        // same constant: `if (a < 0) ...; if (a == 0) ...` shares `cmpwi r3,0`, the second guard
        // branching on the same result (`bne`). That cross-guard condition-register reuse is not
        // modeled — each guard here emits its own compare — so a sequence containing such a pair
        // would emit a redundant second `cmpwi` (a byte diff). Defer it rather than ship that.
        let guard_count = guards.len();
        for (pair_index, pair) in guards.windows(2).enumerate() {
            if let (Some(first), Some(second)) =
                (guard_comparison_key(&pair[0].condition), guard_comparison_key(&pair[1].condition))
            {
                if first == second {
                    // When the SECOND guard of the pair is the LAST guard, it folds with the final
                    // return into a select (the `is_last` path below). If that select lowers
                    // branchlessly (sign-mask `srawi`/`srwi`, or a consecutive-constant sign select)
                    // it emits NO compare, so the shared key produces no redundant compare and no
                    // cross-guard CR reuse is needed — mwcc emits one compare for the earlier guard
                    // and the branchless tail (e.g. `if(a>0)return 1; if(a<0)return -1; return 0;` ->
                    // `cmpwi;ble;li 1;blr; srawi;blr`; or a `> 0 ? 2 : 3` tail -> `neg;andc;srawi;
                    // addi`). Compare-based tails (==0/!=0/<=0/variable) are NOT branchless here and
                    // keep deferring (they also defer in evaluate_tail), so no DIFF is shipped.
                    let second_is_last = pair_index + 2 == guard_count;
                    if second_is_last && (!final_in_result || constant_value(&pair[1].value).is_some()) {
                        let select = guard_select(&pair[1].condition, &pair[1].value, final_return);
                        if let Expression::Conditional { condition, when_true, when_false } = &select {
                            if crate::control_flow::select_folds_branchless(condition, when_true, when_false) {
                                continue;
                            }
                        }
                    }
                    return Err(Diagnostic::error("consecutive guards sharing a compare need cross-guard CR reuse (roadmap)"));
                }
            }
        }

        for (index, guard) in guards.iter().enumerate() {
            let is_last = index + 1 == guards.len();

            // A null-guarded dereference `if (!p) return CONST; return *p;` cannot fold branchless
            // (dereferencing null is unsafe); mwcc emits a real branch with the deref in the
            // fall-through and the constant as the cold tail: `cmplwi p,0; beq COLD; <*p>; blr;
            // COLD: li CONST; blr`.
            if is_last {
                if let Some((pointer, hot, cold)) = guarded_null_dereference(&guard.condition, &guard.value, final_return, return_type) {
                    if let Some(pointer_register) = self.lookup_general(pointer) {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: pointer_register, immediate: 0 });
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 });
                        self.evaluate_tail(hot, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        let cold_label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = cold_label;
                        }
                        self.evaluate_tail(cold, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
            }

            // mwcc compiles the final guard together with the fall-through return as
            // one branchless select `(cond) ? value : final` — the same form as a
            // lone guard — not a third early-return branch. Earlier guards stay as
            // forward-branching early returns.
            // The last guard folds into the fall-through as a single select `(cond) ? value :
            // final` whenever the final isn't already in the result register, OR the guard value
            // is a constant (the select's constant-arm forms cover `(a>10) ? 1 : a` etc., which
            // the in-result `bnelr` path below cannot — it needs a register value).
            if is_last && (!final_in_result || constant_value(&guard.value).is_some()) {
                let select = guard_select(&guard.condition, &guard.value, final_return);
                self.evaluate_tail(&select, return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            // A (non-last) guard with a CONSTANT value: forward-branch past the return when the
            // condition is false, load the constant into the result, and return —
            // `cmpwi; bge skip; li result, c; blr; skip:`. (A constant has no leaf register, so the
            // leaf paths below would defer at general_register_of_leaf.)
            if let Some(constant) = constant_value(&guard.value) {
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.load_integer_constant(result, constant);
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let next = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = next;
                }
                continue;
            }
            let value_register = self.general_register_of_leaf(&guard.value)?;

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            // A non-last guard whose value already sits in the result register is a
            // conditional return falling through to the next guard (mwcc: `cmpwi; bnelr`),
            // not a forward branch over the return.
            if result == value_register {
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                continue;
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.output.instructions.push(Instruction::move_register(result, value_register));
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
            // De Morgan: `return !(X && Y)` is `!X || !Y` and `!(X || Y)` is `!X && !Y` —
            // mwcc folds the negation into the short-circuit exits rather than computing the
            // operator into a register and inverting it (cntlzw/srwi). Single level only;
            // a nested logical operand defers to the general path.
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand }
                if matches!(operand.as_ref(), Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. }) =>
            {
                let Expression::Binary { operator: inner, left, right } = operand.as_ref() else { unreachable!() };
                let is_logical = |expression: &Expression| {
                    matches!(expression, Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. })
                };
                if is_logical(left.as_ref()) || is_logical(right.as_ref()) {
                    return Err(Diagnostic::error("a nested negated logical needs the general short-circuit (roadmap)"));
                }
                let flipped = if *inner == BinaryOperator::LogicalAnd { BinaryOperator::LogicalOr } else { BinaryOperator::LogicalAnd };
                let not_left = Expression::Unary { operator: UnaryOperator::LogicalNot, operand: Box::new(left.as_ref().clone()) };
                let not_right = Expression::Unary { operator: UnaryOperator::LogicalNot, operand: Box::new(right.as_ref().clone()) };
                self.emit_short_circuit(flipped, &not_left, &not_right, result)
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
            // A signed narrow local (char/short) returned at a wider type must be
            // sign-extended — `char c = *s; return c;` is `lbz; extsb` like the direct
            // `return *s`. Evaluating the initializer at the local's own narrow type drops
            // that widening, and whether the value is already extended depends on the
            // initializer (a global narrow load appends extsb/lha; a char* deref's `lbz`
            // and a parameter leave the raw byte). Defer the not-already-extended cases
            // rather than return a zero-extended byte where a sign-extended char is meant.
            if self.signed_of(local.declared_type)
                && local.declared_type.width() < return_type.width()
                && local.declared_type.width() < 32
            {
                let initializer_extends = match initializer {
                    // A global signed-narrow load appends the extension (lbz+extsb / lha).
                    Expression::Variable(name) => self.globals.contains_key(name.as_str()),
                    // `lha` sign-extends a halfword; `lbz` does not extend a byte.
                    Expression::Dereference { .. } | Expression::Index { .. } | Expression::Member { .. } => {
                        local.declared_type.width() >= 16
                    }
                    _ => false,
                };
                if !initializer_extends {
                    return Err(Diagnostic::error("a signed narrow local returned at a wider type needs a widening coercion (roadmap)"));
                }
            }
            // A NARROWING leaf initializer — `char c = a;` for a wider `a` — truncates to the
            // narrow type. Inlining it into the return drops that truncation (and the char
            // return's sign-extension): mwcc emits `extsb r3,r3` for `char f(int a){ char c =
            // a; return c; }`, ours returned the raw int. Defer the narrowing.
            if local.declared_type.width() < 32 {
                if let Ok((_, init_width, _)) = self.leaf_info(initializer) {
                    if init_width as u32 > local.declared_type.width() as u32 {
                        return Err(Diagnostic::error("a narrowing narrow local (char/short from a wider value) returned is not supported yet (roadmap)"));
                    }
                }
            }
            return self.evaluate(initializer, local.declared_type, result);
        }

        // An additively-defined local used as an operand of an addition
        // (`int t = a + b; return t + c;`) is one mwcc keeps in a register and
        // mutates in place (`add r3,r3,r4; add r3,r3,r5`); our leaf-in-scratch
        // lowering would instead reassociate it like a direct sum. Defer that exact
        // shape (a `+`-init local feeding a `+`); the allocator will later make it
        // byte-exact. Other shapes (`*` init, or a `*`/`-` use) already match.
        fn feeds_an_addition(name: &str, expression: &Expression) -> bool {
            let is_local = |operand: &Expression| matches!(operand, Expression::Variable(variable) if variable == name);
            match expression {
                Expression::Binary { operator, left, right } => {
                    (*operator == BinaryOperator::Add && (is_local(left) || is_local(right)))
                        || feeds_an_addition(name, left)
                        || feeds_an_addition(name, right)
                }
                Expression::Unary { operand, .. } | Expression::Cast { operand, .. } | Expression::AddressOf { operand } => feeds_an_addition(name, operand),
                Expression::Conditional { condition, when_true, when_false } => {
                    feeds_an_addition(name, condition) || feeds_an_addition(name, when_true) || feeds_an_addition(name, when_false)
                }
                Expression::Dereference { pointer } => feeds_an_addition(name, pointer),
                Expression::Index { base, index } => feeds_an_addition(name, base) || feeds_an_addition(name, index),
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => feeds_an_addition(name, base),
                Expression::Assign { target, value } => feeds_an_addition(name, target) || feeds_an_addition(name, value),
                Expression::Comma { left, right } => feeds_an_addition(name, left) || feeds_an_addition(name, right),
                Expression::Call { arguments, .. } => arguments.iter().any(|argument| feeds_an_addition(name, argument)),
                Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => false,
            }
        }
        if matches!(initializer, Expression::Binary { operator: BinaryOperator::Add, .. })
            && feeds_an_addition(&local.name, return_expression)
        {
            return Err(Diagnostic::error("an additively-defined local used in a sum needs the register allocator to match mwcc's in-place mutation (roadmap)"));
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
        let stride = pointer_stride(local.declared_type);
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed, width: local.declared_type.width(), pointee, stride });
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
                // An integer memory load (`*p`, `a[i]`, `s.member` of integer type) in a
                // float context needs the loaded value run through the int->float conversion.
                // That path is not wired, so defer rather than hand it to evaluate_float,
                // which would mis-evaluate the integer as a float and load it into the GPR
                // whose NUMBER matches the float destination (f1 -> r1, clobbering the stack
                // pointer). Float-typed loads fall through to evaluate_float as before.
                // A deref/index of a leaf-variable base (int pointer, int global array) whose
                // loaded value is not float, or a direct integer struct member. Member-based
                // bases (`*p->fq`, `p->e[i]`) are left to evaluate_float — is_float_value
                // cannot resolve them, and those float loads are already byte-exact.
                let integer_memory_load = match expression {
                    Expression::Dereference { pointer } => {
                        matches!(pointer.as_ref(), Expression::Variable(_)) && !self.is_float_value(expression)
                    }
                    Expression::Index { base, .. } => {
                        matches!(base.as_ref(), Expression::Variable(_)) && !self.is_float_value(expression)
                    }
                    Expression::Member { member_type, .. } => !matches!(member_type, Type::Float | Type::Double),
                    _ => false,
                };
                if integer_memory_load {
                    return Err(Diagnostic::error("an integer memory load in a float context needs an int->float conversion (roadmap)"));
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
