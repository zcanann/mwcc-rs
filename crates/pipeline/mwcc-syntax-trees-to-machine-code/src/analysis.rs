//! Pure predicates and shape queries over expressions — no `Generator` state.

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type, UnaryOperator};
use std::collections::HashSet;

/// Whether the function reads a register-resident value (a parameter or a
/// register local) at a point where a call has already run — which would read it
/// from a caller-saved register the call clobbered. mwcc spills such a value to a
/// callee-saved register (r31…); until that allocator exists, the straight-line
/// non-leaf path must DEFER these rather than emit a read of the clobbered
/// register (a silent miscompile). Conservative: it only clears reads that are
/// guaranteed to happen before every call.
pub(crate) fn reads_value_across_call(function: &Function) -> bool {
    let mut registers: HashSet<&str> = HashSet::new();
    for parameter in &function.parameters {
        registers.insert(parameter.name.as_str());
    }
    for local in &function.locals {
        registers.insert(local.name.as_str());
    }

    // Items run in order: local initializers, then body statements, then the
    // return expression. `prior_call` becomes true once a strictly-earlier item
    // performed a call — after which any register-resident read is clobbered.
    let mut prior_call = false;
    let mut defined_after_last_call: HashSet<&str> = HashSet::new();
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            let mut unsafe_registers = registers.clone();
            unsafe_registers.retain(|name| !defined_after_last_call.contains(name));
            if expression_reads_across_call(initializer, prior_call, &unsafe_registers) {
                return true;
            }
            if expression_has_call(initializer) {
                prior_call = true;
                defined_after_last_call.clear();
            }
            defined_after_last_call.insert(local.name.as_str());
        }
    }
    for statement in &function.statements {
        let mut unsafe_registers = registers.clone();
        unsafe_registers.retain(|name| !defined_after_last_call.contains(name));
        if statement_reads_across_call(statement, prior_call, &unsafe_registers) {
            return true;
        }
        if statement_has_call(statement) {
            prior_call = true;
            defined_after_last_call.clear();
        }
        if let Statement::Assign { name, .. } = statement {
            defined_after_last_call.insert(name.as_str());
        }
    }
    if let Some(value) = &function.return_expression {
        let mut unsafe_registers = registers;
        unsafe_registers.retain(|name| !defined_after_last_call.contains(name));
        if expression_reads_across_call(value, prior_call, &unsafe_registers) {
            return true;
        }
    }
    false
}

/// The register-resident values (parameters/locals) read after a call, in order
/// of first such read — the values mwcc keeps in callee-saved registers across the
/// call. Returns `None` when a value is read across a call *within* one expression
/// (a call beside a register read in a binary/index tree); those need the general
/// allocator and are deferred by the simple callee-saved path.
pub(crate) fn values_live_across_call(function: &Function) -> Option<Vec<String>> {
    let mut registers: HashSet<&str> = HashSet::new();
    for parameter in &function.parameters {
        registers.insert(parameter.name.as_str());
    }
    for local in &function.locals {
        registers.insert(local.name.as_str());
    }

    let mut collected: Vec<String> = Vec::new();
    let mut prior_call = false;
    let take = |expression: &Expression, prior_call: bool, collected: &mut Vec<String>| -> bool {
        if prior_call {
            collect_register_reads(expression, &registers, collected);
            true
        } else {
            !reads_register_after_call(expression, &registers)
        }
    };

    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            if !take(initializer, prior_call, &mut collected) {
                return None;
            }
            if expression_has_call(initializer) {
                prior_call = true;
            }
        }
    }
    for statement in &function.statements {
        let expressions: Vec<&Expression> = match statement {
            Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
                vec![]
            }
            Statement::Store { target, value } => vec![target, value],
            Statement::Assign { value, .. } => vec![value],
            Statement::Expression(expression) => vec![expression],
            Statement::Return(value) => value.iter().collect(),
            Statement::If { .. } | Statement::Switch { .. } | Statement::Loop { .. } => {
                return None
            }
        };
        for expression in expressions {
            if !take(expression, prior_call, &mut collected) {
                return None;
            }
        }
        if statement_has_call(statement) {
            prior_call = true;
        }
    }
    if let Some(value) = &function.return_expression {
        if !take(value, prior_call, &mut collected) {
            return None;
        }
    }
    Some(collected)
}

/// Whether `expression` reads the variable `name`.
pub(crate) fn expression_reads_name(expression: &Expression, name: &str) -> bool {
    let mut single = HashSet::new();
    single.insert(name);
    reads_register(expression, &single)
}

/// Whether executable source in `function` refers to a local or parameter.
/// Declarations remain in the AST for debug provenance even when optimization
/// removes their storage, so frame planning must distinguish declaration from use.
pub(crate) fn function_uses_name(function: &Function, name: &str) -> bool {
    fn arm_uses_name(arm: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
        match arm {
            mwcc_syntax_trees::ArmBody::Return(value) => expression_reads_name(value, name),
            mwcc_syntax_trees::ArmBody::Statements(statements) => {
                statements_use_name(statements, name)
            }
        }
    }

    fn statements_use_name(statements: &[Statement], name: &str) -> bool {
        statements.iter().any(|statement| match statement {
            Statement::Store { target, value } => {
                expression_reads_name(target, name) || expression_reads_name(value, name)
            }
            Statement::Assign { name: assigned, value } => {
                assigned == name || expression_reads_name(value, name)
            }
            Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                expression_reads_name(expression, name)
            }
            Statement::If { condition, then_body, else_body } => {
                expression_reads_name(condition, name)
                    || statements_use_name(then_body, name)
                    || statements_use_name(else_body, name)
            }
            Statement::Switch { scrutinee, arms, default } => {
                expression_reads_name(scrutinee, name)
                    || arms.iter().any(|arm| arm_uses_name(&arm.body, name))
                    || default.as_ref().is_some_and(|arm| arm_uses_name(arm, name))
            }
            Statement::Loop { initializer, condition, step, body, .. } => {
                initializer.iter().chain(condition).chain(step)
                    .any(|expression| expression_reads_name(expression, name))
                    || statements_use_name(body, name)
            }
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
        })
    }

    function.locals.iter().filter_map(|local| local.initializer.as_ref())
        .any(|initializer| expression_reads_name(initializer, name))
        || statements_use_name(&function.statements, name)
        || function.guards.iter().any(|guard| {
            expression_reads_name(&guard.condition, name)
                || expression_reads_name(&guard.value, name)
        })
        || function.return_expression.as_ref()
            .is_some_and(|value| expression_reads_name(value, name))
}

/// Whether evaluating `expression` can read `name` after a real call has
/// completed. `prior_call` carries call state from the enclosing statement
/// sequence; calls nested inside the expression are evaluated in the order
/// modeled by [`reads_register_after_call`].
pub(crate) fn expression_reads_name_across_call(
    expression: &Expression,
    name: &str,
    prior_call: bool,
) -> bool {
    let mut single = HashSet::new();
    single.insert(name);
    expression_reads_across_call(expression, prior_call, &single)
}

/// Count every textual read of the variable `name` within `expression` (not de-duplicated).
/// Used to detect a value that would be materialized at more than one site if inlined.
pub(crate) fn count_name_occurrences(expression: &Expression, name: &str) -> usize {
    match expression {
        // A compound literal is a constant image — no reads.
        Expression::CompoundLiteral { .. } => 0,
        Expression::CallThrough { target, arguments } => {
            count_name_occurrences(target, name)
                + arguments
                    .iter()
                    .map(|argument| count_name_occurrences(argument, name))
                    .sum::<usize>()
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            count_name_occurrences(object, name)
                + arguments
                    .iter()
                    .map(|argument| count_name_occurrences(argument, name))
                    .sum::<usize>()
        }
        Expression::ConstructedNew { arguments, .. } => arguments
            .iter()
            .map(|argument| count_name_occurrences(argument, name))
            .sum(),
        Expression::AggregateLiteral(_) => 0,
        Expression::Variable(variable) => usize::from(variable == name),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => 0,
        Expression::Binary { left, right, .. } => {
            count_name_occurrences(left, name) + count_name_occurrences(right, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            count_name_occurrences(operand, name)
        }
        Expression::PostStep { target, .. } => 2 * count_name_occurrences(target, name),
        Expression::Dereference { pointer } => count_name_occurrences(pointer, name),
        Expression::AddressOf { operand } => count_name_occurrences(operand, name),
        Expression::Index { base, index } => {
            count_name_occurrences(base, name) + count_name_occurrences(index, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            count_name_occurrences(base, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            count_name_occurrences(condition, name)
                + count_name_occurrences(when_true, name)
                + count_name_occurrences(when_false, name)
        }
        Expression::Call {
            name: callee,
            arguments,
        } => {
            usize::from(callee == name)
                + arguments
                    .iter()
                    .map(|argument| count_name_occurrences(argument, name))
                    .sum::<usize>()
        }
        Expression::Assign { target, value } => {
            count_name_occurrences(target, name) + count_name_occurrences(value, name)
        }
        Expression::Comma { left, right } => {
            count_name_occurrences(left, name) + count_name_occurrences(right, name)
        }
    }
}

/// The maximum ARITHMETIC nesting depth at which `name` appears (through Binary/Unary/Cast operators),
/// or `None` if it does not appear on such a path. `x` -> 0, `-x` / `x&C` / `x+f()` -> 1, `(-x)&C` -> 2.
/// A depth >= 2 means the value flows through more than one instruction, so it DIES mid-computation and
/// mwcc interleaves its callee-saved restore at that death point (restore-by-register-death) — which the
/// all-restores-at-end callee-saved epilogue does not model.
pub(crate) fn name_nesting_depth(expression: &Expression, name: &str) -> Option<usize> {
    match expression {
        Expression::Variable(variable) if variable == name => Some(0),
        Expression::Binary { left, right, .. } => [
            name_nesting_depth(left, name),
            name_nesting_depth(right, name),
        ]
        .into_iter()
        .flatten()
        .max()
        .map(|depth| depth + 1),
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            name_nesting_depth(operand, name).map(|depth| depth + 1)
        }
        Expression::BitFieldRead { extracted, .. } => name_nesting_depth(extracted, name),
        _ => None,
    }
}

/// The length of the CONNECTED add-chain rooted at this node (the add-tree mwcc reassociates). A
/// non-add operand (a `*`, a leaf) terminates the chain, so `(a+b)*c + a` is a 1-add chain
/// (byte-exact) — the `a+b` is consumed by the `*c` into a single value — not a 2-add tree.
pub(crate) fn count_adds(expression: &Expression) -> usize {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => 1 + count_adds(left) + count_adds(right),
        _ => 0,
    }
}

/// A bare register/constant leaf, for add-tree shape classification.
fn is_add_leaf(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_) | Expression::IntegerLiteral(_)
    )
}

/// The leaves of an all-`+` chain of bare leaves, in source order: `Some([v1, v2, …, vN])` for a
/// left-associated `(((v1 + v2) + v3) + …) + vN` where every operand is a leaf, else `None`. mwcc
/// reassociates such a chain to `v1 + left-fold(v2..vN)`, which the codegen reproduces directly.
pub(crate) fn add_chain_leaves(expression: &Expression) -> Option<Vec<&Expression>> {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => {
            if !is_add_leaf(right) {
                return None;
            }
            let mut leaves = add_chain_leaves(left)?;
            leaves.push(right);
            Some(leaves)
        }
        _ if is_add_leaf(expression) => Some(vec![expression]),
        _ => None,
    }
}

/// An integer `Add` that mwcc REASSOCIATES and our register allocator does not match byte-for-byte:
/// a tree of >= 2 additions that is NOT the simple left-associated `(leaf + leaf) + leaf` form.
/// Byte-exact and kept: `a+b`, `a+b+c`, `a+b*c`, `a*b+c*d`, `(a+b+c)*d`. Diverges: `a+b+c+d`,
/// `a+(b+c)`, `a+b+c*d`, `d+(a+b+c)` — mwcc evaluates the nested-add operand in its own order.
pub(crate) fn is_complex_add(expression: &Expression) -> bool {
    // `(a - 1) + a` / `(a + 1) + b` / `b + (a - 1)`: mwcc HOISTS the embedded additive
    // constant to the end, grouping the two register terms first (`add r3,r3,r3; addi
    // r3,r3,-1` for `(a-1)+a`). This is a 1-`Add` tree (the constant lives in a `+`/`-`
    // child), so `count_adds` misses it, but the source-order codegen still diverges.
    if is_constant_hoist_add(expression) {
        return true;
    }
    // `(a+b)-1`: mwcc reassociates a `sum - const`, pushing the constant into the sum's SECOND
    // operand (`a+(b-1)` = `mr r0,r3; addi r3,r4,-1; add r3,r0,r3`); our source-order `add; addi`
    // diverges. The equivalent `a+(b-1)` already defers via is_constant_hoist_add, but the
    // Subtract-outer spelling escapes the Add-focused checks below, so catch it here. Restricted to
    // a TWO-register inner sum: `(a+10)-3` has a constant inner operand and constant-FOLDS to
    // `addi r3,r3,7` (byte-exact, driver.rs), `(a-b)-1` keeps source order, `(a+b)-c` is not a hoist.
    if let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    {
        if matches!(right.as_ref(), Expression::IntegerLiteral(_)) {
            if let Expression::Binary {
                operator: BinaryOperator::Add,
                right: inner_right,
                ..
            } = left.as_ref()
            {
                if !matches!(inner_right.as_ref(), Expression::IntegerLiteral(_)) {
                    return true;
                }
            }
        }
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return false;
    };
    if count_adds(expression) < 2 {
        return false;
    }
    // The one byte-exact >= 2-add shape: `(leaf + leaf) + (leaf | const)`.
    let simple = matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right }
        if is_add_leaf(inner_left) && is_add_leaf(inner_right))
        && is_add_leaf(right);
    !simple
}

/// An `Add` mwcc reassociates by HOISTING embedded additive constants to the end: `(a-1)+a` ->
/// `(a+a)-1` (`add r3,r3,r3; addi r3,r3,-1`), `(a+1)+b`, `b+(a-1)`, and `(a-1)+(b-1)` -> `(a+b)-2`
/// (the constants are summed). One operand is a `+`/`-` binary whose RIGHT operand is a CONSTANT
/// (`X ± c`); the OTHER operand is a bare register leaf OR another such `Y ± c`. mwcc groups the
/// register terms and applies the summed constant last, which the source-order codegen does not
/// reproduce. Excluded (all match mwcc): a computed non-additive other-operand (`(a-1)+b*c`), a
/// constant already at the outer position (`(a*a)+3`, `(a-b)+1`), and no inner constant (`(a-b)+a`).
fn is_constant_hoist_add(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return false;
    };
    let register_leaf = |operand: &Expression| matches!(operand, Expression::Variable(_));
    (additive_with_constant(left) && (register_leaf(right) || additive_with_constant(right)))
        || (additive_with_constant(right) && register_leaf(left))
}

/// A `variable ± constant` binary (`a + c`, `a - c`) — a bare register leaf plus an integer
/// constant, the shape whose embedded constant mwcc hoists during reassociation. A COMPUTED
/// left operand (`(a*b) + 1`) is a single materialized value, not a hoistable term, so mwcc
/// keeps it in place (`((a*b)+1)*c` and `((a*b)+1)+c` match) — hence the `Variable` left.
fn additive_with_constant(operand: &Expression) -> bool {
    matches!(operand, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
        if matches!(left.as_ref(), Expression::Variable(_)) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
}

/// A value materialized into a scratch register from ONE register leaf via an immediate or unary
/// op: `a ± const` / `const ± a` (addi/subfic) or `-a` (neg). mwcc keeps such an operand in source
/// order (as the FIRST/rA operand) in a commutative op — `(a-1)*b` -> `mullw r3,r0,r4`; our default
/// placement anchors the leaf instead (`mullw r3,r4,r0`). `place_general_operands` orders it
/// computed-first to match. A TWO-register op (`a+b`, `a-c`) or a nested product (`(a*b)+1`) is a
/// different shape and is NOT this.
pub(crate) fn single_register_computed(operand: &Expression) -> bool {
    match operand {
        Expression::Binary {
            operator: BinaryOperator::Add | BinaryOperator::Subtract,
            left,
            right,
        } => {
            (matches!(left.as_ref(), Expression::Variable(_))
                && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
                || (matches!(left.as_ref(), Expression::IntegerLiteral(_))
                    && matches!(right.as_ref(), Expression::Variable(_)))
        }
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => matches!(operand.as_ref(), Expression::Variable(_)),
        _ => false,
    }
}

/// Whether an integer expression CONTAINS a reassociated add-tree anywhere — the whole expression
/// then defers, since the divergence is in register allocation (after instruction selection).
pub(crate) fn contains_complex_add(expression: &Expression) -> bool {
    if is_complex_add(expression) {
        return true;
    }
    match expression {
        Expression::Binary { left, right, .. } => {
            contains_complex_add(left) || contains_complex_add(right)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => contains_complex_add(operand),
        Expression::Index { base, index } => {
            contains_complex_add(base) || contains_complex_add(index)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            contains_complex_add(condition)
                || contains_complex_add(when_true)
                || contains_complex_add(when_false)
        }
        _ => false,
    }
}

/// A constant-amount shift (`a << 2`, `a >> 3`). mwcc keeps such a shift as the FIRST operand of a
/// commutative op (`(a<<2)+b` -> `add d, shift, b`); our placement swaps it to second (matching the
/// strength-reduced `(a*4)+b` instead). A variable-amount shift, or a shift on the right, matches.
pub(crate) fn is_constant_shift(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator: BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight, right, .. }
        if constant_value(right).is_some())
}

/// Whether an integer expression contains a commutative op whose LEFT operand is a constant-shift —
/// our operand placement orders it backwards from mwcc, so defer rather than emit the swapped bytes.
pub(crate) fn contains_commutative_shift_left(expression: &Expression) -> bool {
    if let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    {
        // A CONSTANT right operand fuses (`(x>>n) & const` -> a single `rlwinm`), which is byte-exact;
        // only a non-constant right operand takes the swapped add/or/and/xor/mul order that diverges.
        // A register-LEAF right (`(a<<2) + b`) is now ordered shift-first by place_general_operands
        // (byte-exact), so only a non-leaf right (`(a<<2) + (b<<2)`, a memory/computed operand) still
        // defers — those route through a different placement path that keeps the swapped order.
        if matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Multiply
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor
        ) && is_constant_shift(left)
            && constant_value(right).is_none()
            && !is_add_leaf(right)
        {
            return true;
        }
    }
    match expression {
        Expression::Binary { left, right, .. } => {
            contains_commutative_shift_left(left) || contains_commutative_shift_left(right)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            contains_commutative_shift_left(operand)
        }
        Expression::Index { base, index } => {
            contains_commutative_shift_left(base) || contains_commutative_shift_left(index)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            contains_commutative_shift_left(condition)
                || contains_commutative_shift_left(when_true)
                || contains_commutative_shift_left(when_false)
        }
        _ => false,
    }
}

/// Append (in evaluation order, de-duplicated) every register-resident name read
/// within `expression`.
fn collect_register_reads(
    expression: &Expression,
    registers: &HashSet<&str>,
    collected: &mut Vec<String>,
) {
    match expression {
        Expression::CompoundLiteral { .. } => {}
        Expression::CallThrough { target, arguments } => {
            collect_register_reads(target, registers, collected);
            for argument in arguments {
                collect_register_reads(argument, registers, collected);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_register_reads(object, registers, collected);
            for argument in arguments {
                collect_register_reads(argument, registers, collected);
            }
        }
        Expression::ConstructedNew { arguments, .. } => {
            for argument in arguments {
                collect_register_reads(argument, registers, collected);
            }
        }
        Expression::AggregateLiteral(_) => {}
        Expression::PostStep { target, .. } => collect_register_reads(target, registers, collected),
        Expression::Variable(name) => {
            if registers.contains(name.as_str()) && !collected.iter().any(|seen| seen == name) {
                collected.push(name.clone());
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => {}
        Expression::Binary { left, right, .. } => {
            collect_register_reads(left, registers, collected);
            collect_register_reads(right, registers, collected);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_register_reads(operand, registers, collected)
        }
        Expression::Dereference { pointer } => {
            collect_register_reads(pointer, registers, collected)
        }
        Expression::AddressOf { operand } => collect_register_reads(operand, registers, collected),
        Expression::Index { base, index } => {
            collect_register_reads(base, registers, collected);
            collect_register_reads(index, registers, collected);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            collect_register_reads(base, registers, collected)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_register_reads(condition, registers, collected);
            collect_register_reads(when_true, registers, collected);
            collect_register_reads(when_false, registers, collected);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                collect_register_reads(argument, registers, collected);
            }
        }
        Expression::Assign { target, value } => {
            collect_register_reads(target, registers, collected);
            collect_register_reads(value, registers, collected);
        }
        Expression::Comma { left, right } => {
            collect_register_reads(left, registers, collected);
            collect_register_reads(right, registers, collected);
        }
    }
}

fn statement_reads_across_call(
    statement: &Statement,
    prior_call: bool,
    registers: &HashSet<&str>,
) -> bool {
    match statement {
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
        Statement::Store { target, value } => {
            expression_reads_across_call(target, prior_call, registers)
                || expression_reads_across_call(value, prior_call, registers)
        }
        Statement::Assign { value, .. } => {
            expression_reads_across_call(value, prior_call, registers)
        }
        Statement::Expression(expression) => {
            expression_reads_across_call(expression, prior_call, registers)
        }
        Statement::Return(value) => value
            .as_ref()
            .is_some_and(|value| expression_reads_across_call(value, prior_call, registers)),
        // A branch body is a statement *sequence*: a call in an earlier body
        // statement clobbers a register read by a later one, so it must be sequenced
        // (the condition runs first).
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            if expression_reads_across_call(condition, prior_call, registers) {
                return true;
            }
            let body_prior = prior_call || expression_has_call(condition);
            sequence_reads_across_call(then_body, body_prior, registers)
                || sequence_reads_across_call(else_body, body_prior, registers)
        }
        Statement::Switch { scrutinee, .. } => {
            expression_reads_across_call(scrutinee, prior_call, registers)
        }
        // A loop body re-runs, so a call anywhere in it can clobber a register read
        // on any iteration — treat the whole construct as post-call when it calls.
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            let body_prior = prior_call || body.iter().any(statement_has_call);
            initializer
                .iter()
                .chain(condition)
                .chain(step)
                .any(|e| expression_reads_across_call(e, body_prior, registers))
                || sequence_reads_across_call(body, body_prior, registers)
        }
    }
}

/// Whether a statement *sequence* reads a register value after one of its own
/// calls, propagating `prior_call` across the statements as the top-level driver
/// does for the function body.
fn sequence_reads_across_call(
    statements: &[Statement],
    mut prior_call: bool,
    registers: &HashSet<&str>,
) -> bool {
    let mut defined_after_last_call: HashSet<&str> = HashSet::new();
    for statement in statements {
        let mut unsafe_registers = registers.clone();
        unsafe_registers.retain(|name| !defined_after_last_call.contains(name));
        if statement_reads_across_call(statement, prior_call, &unsafe_registers) {
            return true;
        }
        if statement_has_call(statement) {
            prior_call = true;
            defined_after_last_call.clear();
        }
        if let Statement::Assign { name, .. } = statement {
            defined_after_last_call.insert(name.as_str());
        }
    }
    false
}

/// Whether evaluating `expression` reads a register-resident value after a call.
/// If a call already ran (`prior_call`), any register read is unsafe. Otherwise
/// the read is unsafe only if a call *within* this expression can precede it —
/// arithmetic on a call *result* (`g(a) + 1`) is fine because the register read
/// `a` lives in the call's argument (evaluated before the call) and nothing is
/// read afterward, whereas `a + g()` is not (mwcc evaluates the call operand
/// first, so `a` is read after it).
fn expression_reads_across_call(
    expression: &Expression,
    prior_call: bool,
    registers: &HashSet<&str>,
) -> bool {
    if prior_call {
        return reads_register(expression, registers);
    }
    reads_register_after_call(expression, registers)
}

/// Whether, evaluating `expression`, a register-resident read can happen after a
/// call completes. Binary/index operands may be evaluated in either order (mwcc
/// runs the heavier — a call — first), so a call in one operand beside a register
/// read in the other is unsafe; a call's arguments run before that call, so reads
/// confined to them are safe.
fn reads_register_after_call(expression: &Expression, registers: &HashSet<&str>) -> bool {
    // Two sibling operands evaluated in an order mwcc may pick: a call in one
    // beside a register read in the other can read the register after the call.
    let pair = |left: &Expression, right: &Expression| {
        reads_register_after_call(left, registers)
            || reads_register_after_call(right, registers)
            || (expression_has_call(left) && reads_register(right, registers))
            || (expression_has_call(right) && reads_register(left, registers))
    };
    match expression {
        // An indirect call evaluates its callee `target` and arguments BEFORE the
        // branch, so a NO-ARGUMENT call through a simple pointer target (`(*s->fp)()`,
        // `(**pp)()`) reads nothing after the call — the callee load is the last thing
        // before `bctrl`. With arguments (which collide with the pointer's base
        // register) or a computed target, the interplay needs the allocator: stay
        // conservative.
        Expression::CallThrough { target, arguments } => {
            !arguments.is_empty()
                || !matches!(
                    target.as_ref(),
                    Expression::Dereference { .. } | Expression::Member { .. }
                )
                || reads_register_after_call(target, registers)
        }
        // The object and explicit arguments all run before dispatch. Preserve
        // the ordinary call-argument safety rule for nested calls.
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            let mut argument_called = false;
            for argument in std::iter::once(object.as_ref()).chain(arguments) {
                if argument_called && reads_register(argument, registers) {
                    return true;
                }
                if reads_register_after_call(argument, registers) {
                    return true;
                }
                if expression_has_call(argument) {
                    argument_called = true;
                }
            }
            false
        }
        // Allocation completes before constructor arguments are marshaled, so
        // every register-backed constructor argument crosses a real call.
        Expression::ConstructedNew { arguments, .. } => arguments
            .iter()
            .any(|argument| reads_register(argument, registers)),
        Expression::CompoundLiteral { .. } => false,
        Expression::AggregateLiteral(_) => false,
        Expression::PostStep { target, .. } => {
            matches!(target.as_ref(), Expression::Call { .. }) || expression_has_call(target)
        }
        Expression::Variable(_)
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => false,
        // Short-circuit logical operands have a fixed source order. A call in
        // the right operand cannot retroactively clobber a value read by the
        // left condition; only a call in the left can precede right-side reads.
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
            left,
            right,
        } => {
            reads_register_after_call(left, registers)
                || reads_register_after_call(right, registers)
                || (expression_has_call(left) && reads_register(right, registers))
        }
        Expression::Binary { left, right, .. } => pair(left, right),
        Expression::Index { base, index } => pair(base, index),
        Expression::Assign { target, value } => pair(target, value),
        // The comma operator is a sequence point: left is complete before right.
        Expression::Comma { left, right } => {
            reads_register_after_call(left, registers)
                || reads_register_after_call(right, registers)
                || (expression_has_call(left) && reads_register(right, registers))
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            reads_register_after_call(operand, registers)
        }
        Expression::Dereference { pointer } => reads_register_after_call(pointer, registers),
        Expression::AddressOf { operand } => reads_register_after_call(operand, registers),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            reads_register_after_call(base, registers)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            reads_register_after_call(condition, registers)
                || reads_register_after_call(when_true, registers)
                || reads_register_after_call(when_false, registers)
                || (expression_has_call(condition)
                    && (reads_register(when_true, registers)
                        || reads_register(when_false, registers)))
                || ((expression_has_call(when_true) || expression_has_call(when_false))
                    && reads_register(condition, registers))
        }
        // A call's arguments run left-to-right before the call; a read is unsafe
        // only if an earlier argument already made a call.
        Expression::Call { arguments, .. } => {
            let mut argument_called = false;
            for argument in arguments {
                if argument_called && reads_register(argument, registers) {
                    return true;
                }
                if reads_register_after_call(argument, registers) {
                    return true;
                }
                if expression_has_call(argument) {
                    argument_called = true;
                }
            }
            false
        }
    }
}

/// Whether `expression` reads any register-resident name.
pub(crate) fn reads_register(expression: &Expression, registers: &HashSet<&str>) -> bool {
    match expression {
        Expression::CompoundLiteral { .. } => false,
        Expression::CallThrough { target, arguments } => {
            reads_register(target, registers)
                || arguments
                    .iter()
                    .any(|argument| reads_register(argument, registers))
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            reads_register(object, registers)
                || arguments
                    .iter()
                    .any(|argument| reads_register(argument, registers))
        }
        Expression::ConstructedNew { arguments, .. } => arguments
            .iter()
            .any(|argument| reads_register(argument, registers)),
        Expression::AggregateLiteral(_) => false,
        Expression::PostStep { target, .. } => reads_register(target, registers),
        Expression::Variable(name) => registers.contains(name.as_str()),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => false,
        Expression::Binary { left, right, .. } => {
            reads_register(left, registers) || reads_register(right, registers)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => reads_register(operand, registers),
        Expression::Dereference { pointer } => reads_register(pointer, registers),
        Expression::AddressOf { operand } => reads_register(operand, registers),
        Expression::Index { base, index } => {
            reads_register(base, registers) || reads_register(index, registers)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            reads_register(base, registers)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            reads_register(condition, registers)
                || reads_register(when_true, registers)
                || reads_register(when_false, registers)
        }
        // A call THROUGH a register-resident name (a function-pointer local/param) READS
        // that name — the callee NAME counts, not just the arguments.
        Expression::Call { name, arguments } => {
            registers.contains(name.as_str())
                || arguments
                    .iter()
                    .any(|argument| reads_register(argument, registers))
        }
        Expression::Assign { target, value } => {
            reads_register(target, registers) || reads_register(value, registers)
        }
        Expression::Comma { left, right } => {
            reads_register(left, registers) || reads_register(right, registers)
        }
    }
}

/// Names mwcc lowers to a single instruction rather than an out-of-line call, so
/// they do NOT make a function non-leaf: the `__fabs` floating absolute-value intrinsic.
pub(crate) fn is_intrinsic_call(name: &str) -> bool {
    name == "__fabs"
}

/// Whether an expression contains a call anywhere.
pub(crate) fn expression_has_call(expression: &Expression) -> bool {
    match expression {
        // An intrinsic (`__fabs`) is not a real call, but a real call in its ARGUMENT
        // still makes the function non-leaf, so recurse into the arguments.
        Expression::Call { name, arguments } if is_intrinsic_call(name) => {
            arguments.iter().any(expression_has_call)
        }
        Expression::Call { .. } => true,
        // An indirect call (through a function pointer) always makes the function
        // non-leaf — the link register must be saved around the `bctrl`.
        Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. } => true,
        Expression::Binary { left, right, .. } => {
            expression_has_call(left) || expression_has_call(right)
        }
        Expression::Unary { operand, .. } => expression_has_call(operand),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_has_call(condition)
                || expression_has_call(when_true)
                || expression_has_call(when_false)
        }
        Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => expression_has_call(operand),
        Expression::Assign { target, value }
        | Expression::Comma {
            left: target,
            right: value,
        } => expression_has_call(target) || expression_has_call(value),
        Expression::Dereference { pointer } => expression_has_call(pointer),
        Expression::Index { base, index } => {
            expression_has_call(base) || expression_has_call(index)
        }
        // `get()->field` / `get()->arr[i]`: a call in the member/member-address base still makes
        // the function non-leaf (it must save the link register around the call).
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_has_call(base)
        }
        _ => false,
    }
}

/// Whether `expression` has an observable side effect (a call or an assignment store).
/// Used to decide whether a comma operand can be peeled to its right value or must defer.
pub(crate) fn expression_has_side_effect(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::Assign { .. } => true,
        Expression::Binary { left, right, .. } => {
            expression_has_side_effect(left) || expression_has_side_effect(right)
        }
        Expression::Comma { left, right } => {
            expression_has_side_effect(left) || expression_has_side_effect(right)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => expression_has_side_effect(operand),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_has_side_effect(condition)
                || expression_has_side_effect(when_true)
                || expression_has_side_effect(when_false)
        }
        Expression::Dereference { pointer } => expression_has_side_effect(pointer),
        Expression::Index { base, index } => {
            expression_has_side_effect(base) || expression_has_side_effect(index)
        }
        _ => false,
    }
}

/// Whether a function makes a call (and so needs the non-leaf prologue).
pub(crate) fn statement_has_call(statement: &Statement) -> bool {
    match statement {
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
        Statement::Store { target, value } => {
            expression_has_call(target) || expression_has_call(value)
        }
        Statement::Assign { value, .. } => expression_has_call(value),
        Statement::Expression(expression) => expression_has_call(expression),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_has_call(scrutinee)
                || arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(result) => expression_has_call(result),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => {
                        statements.iter().any(statement_has_call)
                    }
                })
                || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(result) => expression_has_call(result),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => {
                        statements.iter().any(statement_has_call)
                    }
                })
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_has_call(condition) || block_has_call(then_body) || block_has_call(else_body)
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer.as_ref().is_some_and(expression_has_call)
                || condition.as_ref().is_some_and(expression_has_call)
                || step.as_ref().is_some_and(expression_has_call)
                || block_has_call(body)
        }
        Statement::Return(value) => value.as_ref().is_some_and(expression_has_call),
    }
}

pub(crate) fn block_has_call(statements: &[Statement]) -> bool {
    statements.iter().any(statement_has_call)
}

pub(crate) fn function_makes_call(function: &Function) -> bool {
    function.statements.iter().any(statement_has_call)
        || function
            .return_expression
            .as_ref()
            .is_some_and(expression_has_call)
        || function
            .locals
            .iter()
            .any(|local| local.initializer.as_ref().is_some_and(expression_has_call))
        || function
            .guards
            .iter()
            .any(|guard| expression_has_call(&guard.condition) || expression_has_call(&guard.value))
}

pub(crate) fn is_complex(expression: &Expression) -> bool {
    if let Expression::BitFieldRead { extracted, .. } = expression {
        return is_complex(extracted);
    }
    matches!(
        expression,
        Expression::Binary { .. }
            | Expression::Unary { .. }
            | Expression::Conditional { .. }
            | Expression::Cast { .. }
    )
}

/// The Sethi-Ullman register need of an expression: the number of registers
/// needed to evaluate it without spilling. mwcc evaluates the operand with the
/// *higher* need first — the heavier subtree, independent of source order — which
/// is the key to matching its instruction order on asymmetric arithmetic trees
/// (`((b+c)*(d+e)) + a` and `a + ((b+c)*(d+e))` compile identically because the
/// heavy product is always done first). A leaf needs one register; a binary node
/// needs `n+1` when its two operands tie at `n` (the second result must survive
/// while the first is computed), otherwise the larger of the two — the heavier
/// side absorbs the lighter for free. Loads/calls are approximated as leaves;
/// refine when the placement restructure consumes this.
///
pub(crate) fn register_need(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary { left, right, .. } => {
            let left_need = register_need(left);
            let right_need = register_need(right);
            if left_need == right_need {
                left_need + 1
            } else {
                left_need.max(right_need)
            }
        }
        Expression::Unary { operand, .. } => register_need(operand),
        Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => register_need(operand),
        Expression::Conditional {
            when_true,
            when_false,
            ..
        } => register_need(when_true)
            .max(register_need(when_false))
            .max(1),
        _ => 1,
    }
}

/// If `expression` is `*pointer`, the pointer sub-expression.
pub(crate) fn as_dereference(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Dereference { pointer } => Some(pointer),
        _ => None,
    }
}

/// If `expression` is `base->field`, its base, byte offset, and member type.
pub(crate) fn as_member(
    expression: &Expression,
) -> Option<(&Expression, u32, mwcc_syntax_trees::Type)> {
    match expression {
        // Only a plain (non-indexed) member is a simple displacement access; an
        // `a[i].field` (index_stride set) routes through the indexed-load path.
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } => Some((base, *offset, *member_type)),
        _ => None,
    }
}

pub(crate) fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

/// The integer value if `expression` is a literal or a negated literal.
pub(crate) fn constant_value(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value),
        // Fold `-c` and `~c` of a constant operand, so e.g. `x & ~7` becomes a
        // mask immediate rather than falling into a broken two-operand path.
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => constant_value(operand).map(|value| value.wrapping_neg()),
        Expression::Unary {
            operator: UnaryOperator::BitNot,
            operand,
        } => constant_value(operand).map(|value| !value),
        // Preserve casts that do not change the represented integer. This is
        // enough to fold source-level `OSRoundUp32B(sizeof(pointer))` without
        // guessing about narrowing, signed wrap, or pointer conversions.
        Expression::Cast {
            target_type,
            operand,
        } => {
            let value = constant_value(operand)?;
            match target_type {
                Type::Int if i32::try_from(value).is_ok() => Some(value),
                Type::UnsignedInt if u32::try_from(value).is_ok() => Some(value),
                Type::Short if i16::try_from(value).is_ok() => Some(value),
                Type::UnsignedShort if u16::try_from(value).is_ok() => Some(value),
                Type::Char if i8::try_from(value).is_ok() => Some(value),
                Type::UnsignedChar if u8::try_from(value).is_ok() => Some(value),
                // C's null pointer constant remains the all-zero bit pattern
                // after conversion to any object/function pointer type.
                Type::Pointer(_) | Type::StructPointer { .. } if value == 0 => Some(0),
                _ => None,
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            use BinaryOperator::*;
            // `x - x` and `x ^ x` are 0 for any side-effect-free operand, even a
            // non-constant one (mwcc folds them without touching memory).
            if matches!(operator, Subtract | BitXor) && same_operand(left, right) {
                return Some(0);
            }
            // Otherwise fold arithmetic of two compile-time constants (`2 + 3`,
            // `FLAG_A | FLAG_B`, `1 << 3`), matching mwcc's `li`/`lis;ori`. The
            // result is truncated to 32 bits (C `int` arithmetic) so e.g. `1 << 31`
            // is the negative `0x80000000`, materialized by a single `lis`.
            let (l, r) = (constant_value(left)?, constant_value(right)?);
            let folded = match operator {
                Add => l.wrapping_add(r),
                Subtract => l.wrapping_sub(r),
                Multiply => l.wrapping_mul(r),
                BitAnd => l & r,
                BitOr => l | r,
                BitXor => l ^ r,
                ShiftLeft if (0..32).contains(&r) => l.wrapping_shl(r as u32),
                ShiftRight if (0..32).contains(&r) => l >> r,
                _ => return None,
            };
            Some(folded as i32 as i64)
        }
        _ => None,
    }
}

/// Whether two expressions are the SAME side-effect-free value — identical
/// variable, dereference, member, or subscript (recursively). Calls and other
/// effectful nodes never match, so `x - x`/`x == x` style identities are only
/// folded when re-evaluating `x` would be observably identical.
pub(crate) fn same_operand(a: &Expression, b: &Expression) -> bool {
    match (a, b) {
        (Expression::IntegerLiteral(x), Expression::IntegerLiteral(y)) => x == y,
        (Expression::Variable(x), Expression::Variable(y)) => x == y,
        (Expression::Dereference { pointer: pa }, Expression::Dereference { pointer: pb }) => {
            same_operand(pa, pb)
        }
        (
            Expression::Member {
                base: ba,
                offset: oa,
                ..
            },
            Expression::Member {
                base: bb,
                offset: ob,
                ..
            },
        ) => oa == ob && same_operand(ba, bb),
        (
            Expression::Index {
                base: ba,
                index: ia,
            },
            Expression::Index {
                base: bb,
                index: ib,
            },
        ) => same_operand(ba, bb) && same_operand(ia, ib),
        _ => false,
    }
}

/// Full structural equality of two expressions (deeper than [`same_operand`], which stops at
/// leaves/derefs/members). Used to detect a repeated common sub-expression.
pub(crate) fn structurally_equal(a: &Expression, b: &Expression) -> bool {
    match (a, b) {
        (Expression::IntegerLiteral(x), Expression::IntegerLiteral(y)) => x == y,
        (Expression::FloatLiteral(x), Expression::FloatLiteral(y)) => x == y,
        (Expression::StringLiteral(x), Expression::StringLiteral(y)) => x == y,
        (Expression::Variable(x), Expression::Variable(y)) => x == y,
        (
            Expression::Binary {
                operator: oa,
                left: la,
                right: ra,
            },
            Expression::Binary {
                operator: ob,
                left: lb,
                right: rb,
            },
        ) => oa == ob && structurally_equal(la, lb) && structurally_equal(ra, rb),
        (
            Expression::Unary {
                operator: oa,
                operand: pa,
            },
            Expression::Unary {
                operator: ob,
                operand: pb,
            },
        ) => oa == ob && structurally_equal(pa, pb),
        (
            Expression::Conditional {
                condition: ca,
                when_true: ta,
                when_false: fa,
                origin: oa,
            },
            Expression::Conditional {
                condition: cb,
                when_true: tb,
                when_false: fb,
                origin: ob,
            },
        ) => {
            oa == ob
                && structurally_equal(ca, cb)
                && structurally_equal(ta, tb)
                && structurally_equal(fa, fb)
        }
        (
            Expression::Cast {
                target_type: ta,
                operand: pa,
            },
            Expression::Cast {
                target_type: tb,
                operand: pb,
            },
        ) => ta == tb && structurally_equal(pa, pb),
        (
            Expression::BitFieldRead {
                extracted: ea,
                promoted_type: ta,
                ..
            },
            Expression::BitFieldRead {
                extracted: eb,
                promoted_type: tb,
                ..
            },
        ) => ta == tb && structurally_equal(ea, eb),
        (Expression::Dereference { pointer: pa }, Expression::Dereference { pointer: pb }) => {
            structurally_equal(pa, pb)
        }
        (Expression::AddressOf { operand: pa }, Expression::AddressOf { operand: pb }) => {
            structurally_equal(pa, pb)
        }
        (
            Expression::Index {
                base: ba,
                index: ia,
            },
            Expression::Index {
                base: bb,
                index: ib,
            },
        ) => structurally_equal(ba, bb) && structurally_equal(ia, ib),
        (
            Expression::Member {
                base: ba,
                offset: oa,
                member_type: ma,
                index_stride: sa,
            },
            Expression::Member {
                base: bb,
                offset: ob,
                member_type: mb,
                index_stride: sb,
            },
        ) => oa == ob && ma == mb && sa == sb && structurally_equal(ba, bb),
        (
            Expression::MemberAddress {
                base: ba,
                offset: oa,
                element: ea,
                index_stride: sa,
            },
            Expression::MemberAddress {
                base: bb,
                offset: ob,
                element: eb,
                index_stride: sb,
            },
        ) => oa == ob && ea == eb && sa == sb && structurally_equal(ba, bb),
        (
            Expression::Call {
                name: na,
                arguments: aa,
            },
            Expression::Call {
                name: nb,
                arguments: ab,
            },
        ) => {
            na == nb
                && aa.len() == ab.len()
                && aa.iter().zip(ab).all(|(x, y)| structurally_equal(x, y))
        }
        (
            Expression::Assign {
                target: ta,
                value: va,
            },
            Expression::Assign {
                target: tb,
                value: vb,
            },
        ) => structurally_equal(ta, tb) && structurally_equal(va, vb),
        (
            Expression::Comma {
                left: la,
                right: ra,
            },
            Expression::Comma {
                left: lb,
                right: rb,
            },
        ) => structurally_equal(la, lb) && structurally_equal(ra, rb),
        _ => false,
    }
}

/// Whether the expression tree COMPUTES the same arithmetic sub-expression more than once — a
/// common sub-expression mwcc computes once and reuses, but our straight-line codegen recomputes
/// (a byte-different sequence: `(a+1)+(a+1)`, `(a + (a>>31)) ^ (a>>31)`). Only Binary/Unary
/// COMPUTATIONS count: a repeated LOAD (`*p * *p`, `p->a + p->b`, `a[0]==a[0]`) is re-read
/// byte-exactly, matching mwcc, and a leaf is a cheap re-read.
pub(crate) fn has_repeated_nonleaf_subexpression(expression: &Expression) -> bool {
    let mut computed: Vec<&Expression> = Vec::new();
    collect_computed_subexpressions(expression, &mut computed);
    for i in 0..computed.len() {
        for j in (i + 1)..computed.len() {
            if structurally_equal(computed[i], computed[j]) {
                return true;
            }
        }
    }
    false
}

/// Collect every Binary/Unary COMPUTATION node in the tree (recursing through loads, casts, calls,
/// etc. to find nested computations, but not counting those non-arithmetic nodes themselves).
fn collect_computed_subexpressions<'a>(expression: &'a Expression, into: &mut Vec<&'a Expression>) {
    match expression {
        Expression::CompoundLiteral { .. } => {}
        Expression::CallThrough { target, arguments } => {
            collect_computed_subexpressions(target, into);
            for argument in arguments {
                collect_computed_subexpressions(argument, into);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_computed_subexpressions(object, into);
            for argument in arguments {
                collect_computed_subexpressions(argument, into);
            }
        }
        Expression::ConstructedNew { arguments, .. } => {
            for argument in arguments {
                collect_computed_subexpressions(argument, into);
            }
        }
        Expression::AggregateLiteral(_) => {}
        Expression::PostStep { target, .. } => collect_computed_subexpressions(target, into),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_) => {}
        Expression::Binary { left, right, .. } => {
            into.push(expression);
            collect_computed_subexpressions(left, into);
            collect_computed_subexpressions(right, into);
        }
        Expression::Unary { operand, .. } => {
            into.push(expression);
            collect_computed_subexpressions(operand, into);
        }
        Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::AddressOf { operand }
        | Expression::Dereference { pointer: operand } => {
            collect_computed_subexpressions(operand, into);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_computed_subexpressions(condition, into);
            collect_computed_subexpressions(when_true, into);
            collect_computed_subexpressions(when_false, into);
        }
        Expression::Index { base, index } => {
            collect_computed_subexpressions(base, into);
            collect_computed_subexpressions(index, into);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            collect_computed_subexpressions(base, into);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                collect_computed_subexpressions(argument, into);
            }
        }
        Expression::Assign { target, value } => {
            collect_computed_subexpressions(target, into);
            collect_computed_subexpressions(value, into);
        }
        Expression::Comma { left, right } => {
            collect_computed_subexpressions(left, into);
            collect_computed_subexpressions(right, into);
        }
    }
}

/// The variable name if `expression` is a plain variable reference.
pub(crate) fn leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

/// The variable name if `expression` is `~variable`.
pub(crate) fn complemented_leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Unary {
            operator: UnaryOperator::BitNot,
            operand,
        } => leaf_name(operand),
        _ => None,
    }
}

/// Decompose `x & mask` where `x` is a leaf variable and `mask` an integer
/// literal. Returns `(x, mask)` with the mask narrowed to 32 bits.
pub(crate) fn as_masked_leaf(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    leaf_name(left)?;
    constant_value(right).map(|mask| (left.as_ref(), mask as u32))
}

/// Decompose `load & mask` where `load` is a memory load (dereference, member,
/// or index) and `mask` an integer literal. Returns `(load, mask)`.
pub(crate) fn as_masked_load(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if !matches!(
        left.as_ref(),
        Expression::Dereference { .. } | Expression::Member { .. } | Expression::Index { .. }
    ) {
        return None;
    }
    constant_value(right).map(|mask| (left.as_ref(), mask as u32))
}

/// If `mask` is a single contiguous run of set bits, return its PowerPC
/// `[begin, end]` bit span (bit 0 = the most significant bit). Non-contiguous
/// (or wrapping) masks return `None`.
pub(crate) fn mask_to_run(mask: u32) -> Option<(u8, u8)> {
    if mask == 0 {
        return None;
    }
    let begin = mask.leading_zeros() as u8;
    let end = 31 - mask.trailing_zeros() as u8;
    let expected = run_mask(begin, end);
    (expected == mask).then_some((begin, end))
}

/// The 32-bit mask whose set bits are the contiguous run `[begin, end]`
/// (bit 0 = the most significant bit).
pub(crate) fn run_mask(begin: u8, end: u8) -> u32 {
    (0xFFFF_FFFFu32 >> begin) & (0xFFFF_FFFFu32 << (31 - end))
}

/// How one operand of a bitfield merge produces its contiguous masked region.
pub(crate) enum FieldSource {
    ShiftLeft(u8),
    ShiftRight(u8),
    Mask,
}

/// Decompose an expression into a contiguous bit field of a leaf variable: a
/// constant shift (`x << n` / `x >> n`) or a mask (`x & m`). Returns the
/// variable, how the field is produced, and its PowerPC `[begin, end]` span.
pub(crate) fn as_field(expression: &Expression) -> Option<(&Expression, FieldSource, u8, u8)> {
    if let Some((value, is_left, shift)) = as_constant_shift(expression) {
        return Some(if is_left {
            (value, FieldSource::ShiftLeft(shift), 0, 31 - shift)
        } else {
            (value, FieldSource::ShiftRight(shift), shift, 31)
        });
    }
    if let Some((value, mask)) = as_masked_leaf(expression) {
        let (begin, end) = mask_to_run(mask)?;
        return Some((value, FieldSource::Mask, begin, end));
    }
    None
}

/// A nonzero integer literal that fits a signed 16-bit immediate.
pub(crate) fn as_small_integer(expression: &Expression) -> Option<i16> {
    // A nonzero compile-time constant (a literal or a folded expression like
    // `2 + 3`) that fits a signed 16-bit immediate.
    constant_value(expression)
        .filter(|value| *value != 0)
        .and_then(|value| i16::try_from(value).ok())
}

/// Decompose a constant shift of a leaf variable: `x << c` or `x >> c` with
/// `c` in `1..=31`. Returns `(x, is_left_shift, c)`. Used to recognize the
/// rotate idiom `(x << c) | (x >> (32-c))`.
pub(crate) fn as_constant_shift(expression: &Expression) -> Option<(&Expression, bool, u8)> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let is_left = match operator {
        BinaryOperator::ShiftLeft => true,
        BinaryOperator::ShiftRight => false,
        _ => return None,
    };
    leaf_name(left)?;
    match constant_value(right) {
        Some(amount) if (1..=31).contains(&amount) => Some((left, is_left, amount as u8)),
        _ => None,
    }
}

/// The `(BO, BI)` of the branch that fires when `operator` is **true** (cr0 bits:
/// 0=LT, 1=GT, 2=EQ; BO 12 = if-true, 4 = if-false). The negated branch is
/// `(BO ^ 8, BI)`.
pub(crate) fn positive_branch(operator: BinaryOperator) -> (u8, u8) {
    match operator {
        BinaryOperator::Greater => (12, 1),
        BinaryOperator::Less => (12, 0),
        BinaryOperator::GreaterEqual => (4, 0),
        BinaryOperator::LessEqual => (4, 1),
        BinaryOperator::Equal => (12, 2),
        BinaryOperator::NotEqual => (4, 2),
        _ => (12, 2),
    }
}

/// The logical negation of a comparison operator (`==`↔`!=`, `<`↔`>=`, `>`↔`<=`).
pub(crate) fn flip_comparison(operator: BinaryOperator) -> Option<BinaryOperator> {
    Some(match operator {
        BinaryOperator::Equal => BinaryOperator::NotEqual,
        BinaryOperator::NotEqual => BinaryOperator::Equal,
        BinaryOperator::Less => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::Less,
        BinaryOperator::Greater => BinaryOperator::LessEqual,
        BinaryOperator::LessEqual => BinaryOperator::Greater,
        _ => return None,
    })
}

pub(crate) fn is_comparison(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
    )
}

/// The `(BO, BI)` of the conditional branch that skips a comparison's guarded code
/// when the comparison is **false** — the branch that reads a `cmpw`/`cmpwi` result
/// in cr0 (BO: 4 = if-false, 12 = if-true; BI: 0=LT, 1=GT, 2=EQ). `None` for a
/// non-comparison operator. This is the single source of truth shared by the branch
/// emitter (which sets cr0 with a fresh compare) and any handler that reuses an
/// already-set cr0 for a second branch on the same operands (the else-if ladder).
pub(crate) fn false_branch_bo_bi(operator: BinaryOperator) -> Option<(u8, u8)> {
    Some(match operator {
        BinaryOperator::Greater => (4, 1),       // ble
        BinaryOperator::Less => (4, 0),          // bge
        BinaryOperator::GreaterEqual => (12, 0), // blt
        BinaryOperator::LessEqual => (12, 1),    // bgt
        BinaryOperator::Equal => (4, 2),         // bne
        BinaryOperator::NotEqual => (12, 2),     // beq
        _ => return None,
    })
}

/// If `expression` is a multiplication, return its two operands.
pub(crate) fn as_multiplication(expression: &Expression) -> Option<(&Expression, &Expression)> {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } => Some((left, right)),
        _ => None,
    }
}

pub(crate) fn is_commutative(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Add
            | BinaryOperator::Multiply
            | BinaryOperator::BitAnd
            | BinaryOperator::BitOr
            | BinaryOperator::BitXor
    )
}

pub(crate) fn fits_signed_16(value: i64) -> bool {
    (-0x8000..=0x7fff).contains(&value)
}

pub(crate) fn fits_unsigned_16(value: i64) -> bool {
    (0..=0xffff).contains(&value)
}

/// If `value` is a single contiguous run of set bits, return the PowerPC
/// `(mask_begin, mask_end)` for `rlwinm rA,rS,0,begin,end`.
pub(crate) fn contiguous_mask(value: i64) -> Option<(u8, u8)> {
    let mask = value as u32;
    if mask == 0 {
        return None;
    }
    let lowest = mask.trailing_zeros();
    let highest = 31 - mask.leading_zeros();
    let shifted = mask >> lowest;
    if shifted & shifted.wrapping_add(1) != 0 {
        return None; // not a single contiguous run
    }
    Some(((31 - highest) as u8, (31 - lowest) as u8))
}

/// A 32-bit mask representable by a single `rlwinm rA,rS,0,MB,ME` — a contiguous
/// run of set bits, possibly wrapping around bit 31->0 (then `begin > end`, e.g.
/// `x & ~16` clears one bit via `rlwinm 0,28,26`). Returns the `(begin, end)`
/// mask-bit pair, or `None` for an all-clear mask or one with two or more runs.
pub(crate) fn rlwinm_mask(value: i64) -> Option<(u8, u8)> {
    if value as u32 == 0 {
        return None;
    }
    if let Some(run) = contiguous_mask(value) {
        return Some(run);
    }
    // A wrapping run of set bits: its complement is a non-wrapping run. If the
    // cleared bits are the run `[begin, end]`, the set bits run from `end+1`
    // wrapping to `begin-1`.
    let (begin, end) = contiguous_mask(!(value as u32) as i64)?;
    Some(((end + 1) & 31, (begin + 31) & 31))
}

/// Whether evaluating `expression` uses the scratch register at all — true when
/// any binary node has a binary child.
pub(crate) fn needs_scratch(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => {
            is_complex(left) || is_complex(right) || needs_scratch(left) || needs_scratch(right)
        }
        Expression::Unary { operator, operand } => {
            matches!(operator, UnaryOperator::LogicalNot) || needs_scratch(operand)
        }
        Expression::Conditional { .. } => true,
        Expression::Cast { .. } => true,
        Expression::BitFieldRead { extracted, .. } => needs_scratch(extracted),
        _ => false,
    }
}

/// Whether a type is a narrow integer (sub-32-bit), whose values are extended
/// when read and truncated when produced as a result.
pub(crate) fn is_narrow_int(value_type: Type) -> bool {
    matches!(
        value_type,
        Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
    )
}

/// Whether `evaluate_*` can compute `expression` into `destination` using only
/// that register and the scratch register.
pub(crate) fn fits_single_scratch(expression: &Expression, destination_is_scratch: bool) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => match (is_complex(left), is_complex(right)) {
            (false, false) => true,
            (true, false) => fits_single_scratch(left, true),
            (false, true) => fits_single_scratch(right, true),
            // Both operands complex: the left side computes into a fresh virtual
            // the allocator places and the right into the scratch, so this fits
            // even when the result itself lands in the scratch (the temporary is
            // no longer a physical register we must find).
            (true, true) => fits_single_scratch(left, false) && fits_single_scratch(right, true),
        },
        Expression::Unary { operator, operand } => match operator {
            UnaryOperator::LogicalNot => {
                !destination_is_scratch && fits_single_scratch(operand, destination_is_scratch)
            }
            _ => fits_single_scratch(operand, destination_is_scratch),
        },
        // A full-width integer/pointer cast is representation-preserving and its
        // evaluator simply writes the operand into the requested destination.
        // Narrowing and floating casts need additional instructions/registers.
        Expression::Cast { target_type, operand }
            if target_type.width() == 32
                && !matches!(target_type, Type::Float | Type::Double) =>
        {
            fits_single_scratch(operand, destination_is_scratch)
        }
        Expression::Conditional { .. } | Expression::Cast { .. } => false,
        Expression::BitFieldRead { extracted, .. } => {
            fits_single_scratch(extracted, destination_is_scratch)
        }
        _ => true,
    }
}

/// Whether `expression` reads a value from memory (a dereference, subscript, or struct member),
/// possibly nested inside arithmetic. When BOTH operands of a binary need a load, the generic
/// combine interleaves them (`lwz; op; lwz; op; combine`) while mwcc hoists both loads to the top
/// (`lwz; lwz; op; op; combine`) with an allocator-chosen register assignment we do not reproduce —
/// a correct-result mis-schedule, so such shapes defer. Variables (value-tracked into registers)
/// and calls (a separate path) are not memory loads here.
pub(crate) fn contains_memory_load(expression: &Expression) -> bool {
    match expression {
        Expression::Dereference { .. } | Expression::Index { .. } | Expression::Member { .. } => {
            true
        }
        Expression::Binary { left, right, .. } => {
            contains_memory_load(left) || contains_memory_load(right)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => contains_memory_load(operand),
        _ => false,
    }
}

/// A COMPOUND operand that wraps a memory load inside an operation (`p->x*p->x`, `-a[i]`), as
/// opposed to a BARE load (`*p`, `a[i]`, `*(p+1)`). Evaluating a compound-load operand emits the
/// load THEN an op, so when both operands are compound the two loads are not adjacent — the
/// schedule mwcc avoids by hoisting both loads first (the keystone allocator). Two BARE loads keep
/// their loads adjacent (`lwz; lwz; combine`) and stay byte-exact, so they are not compound.
pub(crate) fn is_compound_load(expression: &Expression) -> bool {
    if let Expression::BitFieldRead { extracted, .. } = expression {
        return is_compound_load(extracted);
    }
    matches!(
        expression,
        Expression::Binary { .. } | Expression::Unary { .. } | Expression::Cast { .. }
    ) && contains_memory_load(expression)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cast_null_pointer_remains_a_constant_zero() {
        let null = Expression::Cast {
            target_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
            operand: Box::new(Expression::IntegerLiteral(0)),
        };
        assert_eq!(constant_value(&null), Some(0));
    }

    #[test]
    fn an_unused_array_declaration_is_not_an_executable_use() {
        let mut function = Function {
            return_type: Type::Void,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![],
            statements: vec![],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(!function_uses_name(&function, "scratch"));
        function.statements.push(Statement::Expression(Expression::Variable("scratch".into())));
        assert!(function_uses_name(&function, "scratch"));
    }

    #[test]
    fn full_width_casts_are_transparent_to_scratch_planning() {
        let cast = |expression| Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(expression),
        };
        let difference = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(cast(Expression::Variable("left".into()))),
            right: Box::new(cast(Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset: 4,
                member_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                index_stride: None,
            })),
        };
        assert!(fits_single_scratch(&difference, true));
    }

    fn var(name: &str) -> Expression {
        Expression::Variable(name.to_string())
    }
    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }
    fn add(left: Expression, right: Expression) -> Expression {
        binary(BinaryOperator::Add, left, right)
    }
    fn mul(left: Expression, right: Expression) -> Expression {
        binary(BinaryOperator::Multiply, left, right)
    }

    #[test]
    fn a_leaf_needs_one_register() {
        assert_eq!(register_need(&var("a")), 1);
        assert_eq!(register_need(&Expression::IntegerLiteral(5)), 1);
    }

    #[test]
    fn two_leaves_under_a_binary_need_two() {
        // a + b: equal leaves (1,1) -> 2.
        assert_eq!(register_need(&add(var("a"), var("b"))), 2);
    }

    #[test]
    fn balanced_trees_grow_by_one_per_level() {
        // (a+b)*(c+d): both sides 2, equal -> 3.
        let left = add(var("a"), var("b"));
        let right = add(var("c"), var("d"));
        assert_eq!(register_need(&mul(left, right)), 3);
    }

    #[test]
    fn a_heavier_subtree_absorbs_a_lighter_one_for_free() {
        // a + ((b+c)*(d+e)): leaf (1) vs heavy product (3) -> max = 3, not 4.
        let heavy = mul(add(var("b"), var("c")), add(var("d"), var("e")));
        assert_eq!(register_need(&heavy), 3);
        assert_eq!(register_need(&add(var("a"), heavy.clone())), 3);
        // And the need is the same whichever side the heavy subtree is on — the
        // property that makes mwcc's order independent of source order.
        assert_eq!(register_need(&add(heavy, var("a"))), 3);
    }

    #[test]
    fn the_heavier_operand_is_identifiable_by_comparing_needs() {
        // c + a*b: c (1) lighter than a*b (2); the multiply is evaluated first.
        let product = mul(var("a"), var("b"));
        assert!(register_need(&product) > register_need(&var("c")));
    }

    #[test]
    fn a_call_result_definition_is_not_mistaken_for_a_cross_call_survivor() {
        let call = || Expression::Call {
            name: "produce".into(),
            arguments: Vec::new(),
        };
        let registers = HashSet::from(["result"]);
        let assigned_then_read = [
            Statement::Assign {
                name: "result".into(),
                value: call(),
            },
            Statement::If {
                condition: var("result"),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];
        assert!(!sequence_reads_across_call(
            &assigned_then_read,
            false,
            &registers
        ));

        let clobbered_before_read = [
            Statement::Assign {
                name: "result".into(),
                value: call(),
            },
            Statement::Expression(call()),
            Statement::If {
                condition: var("result"),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];
        assert!(sequence_reads_across_call(
            &clobbered_before_read,
            false,
            &registers
        ));
    }

    #[test]
    fn comma_read_after_conditional_call_is_a_cross_call_survivor() {
        let assertion = Expression::Conditional {
            condition: Box::new(var("object")),
            when_true: Box::new(Expression::IntegerLiteral(0)),
            when_false: Box::new(Expression::Call {
                name: "__assert".into(),
                arguments: Vec::new(),
            }),
            origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
        };
        let expanded_inline_body = Expression::Comma {
            left: Box::new(assertion),
            right: Box::new(Expression::Member {
                base: Box::new(var("object")),
                offset: 0,
                member_type: Type::UnsignedInt,
                index_stride: None,
            }),
        };

        assert!(expression_reads_name_across_call(
            &expanded_inline_body,
            "object",
            false
        ));
        assert!(!expression_reads_name_across_call(
            &expanded_inline_body,
            "unrelated",
            false
        ));
    }

    #[test]
    fn discarded_comma_call_still_makes_a_function_non_leaf() {
        let assertion = Expression::Cast {
            target_type: Type::Void,
            operand: Box::new(Expression::Comma {
                left: Box::new(Expression::Call {
                    name: "report".into(),
                    arguments: Vec::new(),
                }),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
        };
        assert!(expression_has_call(&assertion));
    }

    #[test]
    fn right_short_circuit_call_does_not_make_the_left_value_live_across_it() {
        let registers = HashSet::from(["channel"]);
        let assertion = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(var("channel")),
            right: Box::new(Expression::Comma {
                left: Box::new(Expression::Call {
                    name: "report".into(),
                    arguments: Vec::new(),
                }),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
        };
        assert!(!reads_register_after_call(&assertion, &registers));
    }
}

/// Whether an expression OBSERVES MEMORY — an array element, a dereference, a member,
/// or a global variable read (any name outside `register_names`, the parameters and
/// locals). Such a value is a load: moving it across a call or a store changes what it
/// observes, so the inlining folds must not carry it past either.
pub(crate) fn expression_reads_memory(
    expression: &Expression,
    register_names: &std::collections::HashSet<&str>,
) -> bool {
    match expression {
        Expression::Variable(name) => !register_names.contains(name.as_str()),
        Expression::Index { .. } | Expression::Dereference { .. } | Expression::Member { .. } => {
            true
        }
        Expression::Binary { left, right, .. } => {
            expression_reads_memory(left, register_names)
                || expression_reads_memory(right, register_names)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            expression_reads_memory(operand, register_names)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_reads_memory(condition, register_names)
                || expression_reads_memory(when_true, register_names)
                || expression_reads_memory(when_false, register_names)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .any(|argument| expression_reads_memory(argument, register_names)),
        _ => false,
    }
}
