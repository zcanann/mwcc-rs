//! mwcc's symbol-table ordering for a function's referenced names.
//!
//! mwcc assigns symbol-table indices to the globals and callees a function
//! references by an AST traversal — NOT by `.text` reference (offset) order. The
//! traversal is left-to-right except that a binary node visits its RIGHT operand
//! first when the right is a leaf and the left is compound (so `g*2 + h` registers
//! `h` before `g`). Crucially mwcc groups by KIND: every DATA reference (a global
//! operand) is registered first, in traversal order, then every CALL target — so
//! `g(A); B = 1;` registers `A, B, g`, not `A, g, B`. The object writer assigns
//! this function's external/global symbols in this order.

use mwcc_syntax_trees::{Expression, Function, Statement};

/// The data and call references collected during the traversal, kept apart so the
/// data group can be emitted ahead of the call group.
#[derive(Default)]
struct Names {
    data: Vec<String>,
    calls: Vec<String>,
}

/// The referenced names (globals, callees, and locals — the writer filters to the
/// ones that become symbols) in mwcc's symbol-table order: all data references,
/// then all call targets, deduplicated to first occurrence.
pub(crate) fn referenced_names(function: &Function) -> Vec<String> {
    let mut names = Names::default();
    for statement in &function.statements {
        collect_statement(statement, &mut names);
    }
    for guard in &function.guards {
        collect(&guard.condition, &mut names);
        collect(&guard.value, &mut names);
    }
    if let Some(expression) = &function.return_expression {
        collect(expression, &mut names);
    }
    let mut ordered = names.data;
    ordered.extend(names.calls);
    let mut seen = std::collections::HashSet::new();
    ordered.retain(|name| seen.insert(name.clone()));
    ordered
}

/// A leaf operand — a name or a literal — needs no computation; everything else is
/// compound. The binary visit order keys off this.
fn is_leaf(expression: &Expression) -> bool {
    matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_))
}

/// `target = value`, visited as a binary node.
fn collect_assignment(target: &Expression, value: &Expression, names: &mut Names) {
    if is_leaf(value) && !is_leaf(target) {
        collect(value, names);
        collect(target, names);
    } else {
        collect(target, names);
        collect(value, names);
    }
}

fn collect_statement(statement: &Statement, names: &mut Names) {
    match statement {
        Statement::Store { target, value } => collect_assignment(target, value, names),
        Statement::Assign { value, .. } => collect(value, names),
        Statement::Expression(expression) => collect(expression, names),
        Statement::Switch { scrutinee, arms, default } => {
            collect(scrutinee, names);
            for arm in arms {
                collect(&arm.result, names);
            }
            if let Some(default) = default {
                collect(default, names);
            }
        }
        Statement::If { condition, then_body, else_body } => {
            collect(condition, names);
            for statement in then_body.iter().chain(else_body) {
                collect_statement(statement, names);
            }
        }
        Statement::Loop { initializer, condition, step, body, .. } => {
            for expression in initializer.iter().chain(condition).chain(step) {
                collect(expression, names);
            }
            for statement in body {
                collect_statement(statement, names);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                collect(value, names);
            }
        }
    }
}

fn collect(expression: &Expression, names: &mut Names) {
    match expression {
        Expression::Variable(name) => names.data.push(name.clone()),
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => {}
        Expression::Binary { left, right, .. } => {
            if is_leaf(right) && !is_leaf(left) {
                collect(right, names);
                collect(left, names);
            } else {
                collect(left, names);
                collect(right, names);
            }
        }
        Expression::Unary { operand, .. } => collect(operand, names),
        Expression::Cast { operand, .. } => collect(operand, names),
        Expression::Dereference { pointer } => collect(pointer, names),
        Expression::AddressOf { operand } => collect(operand, names),
        Expression::Index { base, index } => {
            collect(base, names);
            collect(index, names);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => collect(base, names),
        Expression::Conditional { condition, when_true, when_false } => {
            collect(condition, names);
            collect(when_true, names);
            collect(when_false, names);
        }
        // A call registers its arguments (left to right) as data references, then
        // the callee into the call group (emitted after all data references).
        Expression::Call { name, arguments } => {
            for argument in arguments {
                collect(argument, names);
            }
            names.calls.push(name.clone());
        }
        Expression::Assign { target, value } => collect_assignment(target, value, names),
    }
}
