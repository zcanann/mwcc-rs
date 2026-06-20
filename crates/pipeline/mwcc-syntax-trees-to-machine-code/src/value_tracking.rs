//! Local value tracking (copy propagation).
//!
//! mwcc does not keep mutable locals in registers across statements; it tracks
//! each local's current *value* (an expression) and substitutes it at the point
//! of use, then compiles the resulting expression. So `int y = x; y = y + 1;
//! return y;` compiles exactly like `return x + 1;`, and `int y = a + b; int z =
//! y * 2; return z;` like `return (a + b) * 2;`. We reproduce that by inlining
//! locals into the return expression and handing it to the normal codegen.

use std::collections::HashMap;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use mwcc_target::Eabi;
use crate::analysis::function_makes_call;
use crate::generator::*;

impl Generator {
    /// Compile the body by inlining value-tracked locals, when the body is in the
    /// shape this handles. Returns `false` (compile nothing) when the existing
    /// single-local / leaf paths should handle it instead, so those stay
    /// byte-identical. Returns `true` once it has emitted the whole body.
    pub(crate) fn try_value_tracking(&mut self, function: &Function) -> Compilation<bool> {
        // Only take over the cases the straight-line path does not: a reassigned
        // local, or more than one local. A single never-reassigned local keeps the
        // existing handling.
        // Only take over the cases the straight-line path does not: a reassigned
        // local, or more than one local. A single never-reassigned local keeps the
        // existing handling (which computes it once in a register).
        let has_assignment = function.statements.iter().any(|statement| matches!(statement, Statement::Assign { .. }));
        if function.locals.is_empty() || (function.locals.len() == 1 && !has_assignment) {
            return Ok(false);
        }
        // Leaf functions only for now: a non-leaf needs the prologue/frame, which
        // the straight-line path sets up. Defer those (they error honestly there).
        if function_makes_call(function) {
            return Ok(false);
        }

        // Constraints — anything outside the pure-local-arithmetic shape defers.
        if !function.guards.is_empty() {
            return Err(Diagnostic::error("value tracking combined with guards is not supported yet (roadmap)"));
        }
        if function.return_type == Type::Void {
            return Err(Diagnostic::error("value tracking for a void function is not supported yet (roadmap)"));
        }

        // Build each local's current value, in order: a declaration initializes it,
        // a later assignment replaces it. Both substitute the values known so far.
        // Inlining duplicates a local's value at each use; that only matches mwcc
        // when no non-trivial computation is duplicated (mwcc keeps such a value in
        // one register — common-subexpression elimination we do not model). Defer a
        // use that would duplicate a non-leaf value.
        let mut values: HashMap<String, Expression> = HashMap::new();
        for local in &function.locals {
            // An uninitialized local has no value until it is assigned below.
            if let Some(initializer) = &local.initializer {
                guard_no_duplication(initializer, &values)?;
                let value = substitute(initializer, &values);
                values.insert(local.name.clone(), value);
            }
        }
        for statement in &function.statements {
            match statement {
                Statement::Assign { name, value } => {
                    guard_no_duplication(value, &values)?;
                    let value = substitute(value, &values);
                    values.insert(name.clone(), value);
                }
                _ => return Err(Diagnostic::error("value tracking with stores or calls is not supported yet (roadmap)")),
            }
        }

        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
        guard_no_duplication(return_expression, &values)?;
        let inlined = substitute(return_expression, &values);
        let result = match function.return_type {
            Type::Float => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        self.evaluate_tail(&inlined, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

/// Error if substituting `values` into `expression` would duplicate a non-leaf
/// computation (a local whose value is not a leaf appearing more than once).
fn guard_no_duplication(expression: &Expression, values: &HashMap<String, Expression>) -> Compilation<()> {
    for (name, value) in values {
        if !is_leaf_value(value) && count_references(name, expression) > 1 {
            return Err(Diagnostic::error("value tracking would duplicate a computation (needs CSE, roadmap)"));
        }
    }
    Ok(())
}

/// Whether an expression is a leaf (free to duplicate): a variable or literal.
fn is_leaf_value(expression: &Expression) -> bool {
    matches!(expression, Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_))
}

/// Count references to the variable `name` within `expression`.
fn count_references(name: &str, expression: &Expression) -> usize {
    match expression {
        Expression::Variable(variable) => usize::from(variable == name),
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => 0,
        Expression::Binary { left, right, .. } => count_references(name, left) + count_references(name, right),
        Expression::Unary { operand, .. } => count_references(name, operand),
        Expression::Conditional { condition, when_true, when_false } => {
            count_references(name, condition) + count_references(name, when_true) + count_references(name, when_false)
        }
        Expression::Cast { operand, .. } => count_references(name, operand),
        Expression::Dereference { pointer } => count_references(name, pointer),
        Expression::Index { base, index } => count_references(name, base) + count_references(name, index),
        Expression::Member { base, .. } => count_references(name, base),
        Expression::MemberAddress { base, .. } => count_references(name, base),
        Expression::AddressOf { operand } => count_references(name, operand),
        Expression::Assign { target, value } => count_references(name, target) + count_references(name, value),
        Expression::Call { arguments, .. } => arguments.iter().map(|argument| count_references(name, argument)).sum(),
    }
}

/// Replace every value-tracked local in `expression` with its current value,
/// recursively. Names not in `values` (parameters, globals) are left untouched.
fn substitute(expression: &Expression, values: &HashMap<String, Expression>) -> Expression {
    match expression {
        Expression::Variable(name) => values.get(name).cloned().unwrap_or_else(|| expression.clone()),
        Expression::Binary { operator, left, right } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute(left, values)),
            right: Box::new(substitute(right, values)),
        },
        Expression::Unary { operator, operand } => {
            Expression::Unary { operator: *operator, operand: Box::new(substitute(operand, values)) }
        }
        Expression::Conditional { condition, when_true, when_false } => Expression::Conditional {
            condition: Box::new(substitute(condition, values)),
            when_true: Box::new(substitute(when_true, values)),
            when_false: Box::new(substitute(when_false, values)),
        },
        Expression::Cast { target_type, operand } => {
            Expression::Cast { target_type: *target_type, operand: Box::new(substitute(operand, values)) }
        }
        Expression::Dereference { pointer } => Expression::Dereference { pointer: Box::new(substitute(pointer, values)) },
        Expression::AddressOf { operand } => Expression::AddressOf { operand: Box::new(substitute(operand, values)) },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(substitute(base, values)),
            index: Box::new(substitute(index, values)),
        },
        Expression::Member { base, offset, member_type, index_stride } => Expression::Member {
            base: Box::new(substitute(base, values)),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        Expression::MemberAddress { base, offset, element } => Expression::MemberAddress {
            base: Box::new(substitute(base, values)),
            offset: *offset,
            element: *element,
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments.iter().map(|argument| substitute(argument, values)).collect(),
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute(target, values)),
            value: Box::new(substitute(value, values)),
        },
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => expression.clone(),
    }
}
