//! Classify global reads in guarded scalar read-modify-write statements.
//!
//! A guarded call can reuse the condition's global value, but measured O0
//! narrow increments reload the global in the taken arm before updating it.
//! Keep that distinction out of the broad driver precheck.

use super::*;

pub(super) fn guarded_body_read_needs_value_reuse(
    statement: &Statement,
    global: &str,
    global_type: Type,
) -> bool {
    // A struct-valued global is never itself loaded into a scalar register.
    // Reads of its members may need a shared *address* across the branch, but
    // that is owned by the structured global-base/member-address caches.  The
    // scalar guard below must not reject those functions as value-reuse cases.
    if matches!(global_type, Type::Struct { .. }) {
        return false;
    }
    match statement {
        Statement::Expression(expression) => expression_reads_name(expression, global),
        Statement::Store { target, value }
            if is_independently_reloaded_narrow_increment(target, value, global, global_type) =>
        {
            false
        }
        Statement::Store { value, .. } => expression_reads_name(value, global),
        _ => false,
    }
}

fn is_independently_reloaded_narrow_increment(
    target: &Expression,
    value: &Expression,
    global: &str,
    global_type: Type,
) -> bool {
    matches!(global_type, Type::Short | Type::UnsignedShort)
        && matches!(target, Expression::Variable(name) if name == global)
        && matches!(value,
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if name == global)
                && constant_value(right) == Some(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }

    fn increment(name: &str) -> Statement {
        Statement::Store {
            target: variable(name),
            value: Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(variable(name)),
                right: Box::new(Expression::IntegerLiteral(1)),
            },
        }
    }

    #[test]
    fn narrow_increment_reloads_in_the_taken_arm() {
        assert!(!guarded_body_read_needs_value_reuse(
            &increment("state"),
            "state",
            Type::Short,
        ));
    }

    #[test]
    fn call_argument_still_requires_the_guard_value() {
        let statement = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![variable("state")],
        });
        assert!(guarded_body_read_needs_value_reuse(
            &statement,
            "state",
            Type::Short,
        ));
    }

    #[test]
    fn wide_increment_remains_deferred_without_an_oracle() {
        assert!(guarded_body_read_needs_value_reuse(
            &increment("state"),
            "state",
            Type::Int,
        ));
    }

    #[test]
    fn aggregate_member_reads_are_left_to_address_caches() {
        let statement = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![Expression::Member {
                base: Box::new(variable("queue")),
                offset: 4,
                member_type: Type::Int,
                index_stride: None,
            }],
        });
        assert!(!guarded_body_read_needs_value_reuse(
            &statement,
            "queue",
            Type::Struct { size: 32, align: 4 },
        ));
    }
}
