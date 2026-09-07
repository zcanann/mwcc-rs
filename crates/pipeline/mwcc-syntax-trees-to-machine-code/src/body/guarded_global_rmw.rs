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
    let reads_value = |expression: &Expression| {
        if !matches!(global_type, Type::Pointer(_) | Type::StructPointer { .. }) {
            return expression_reads_name(expression, global);
        }
        // A pointer used only as a member address belongs to the structured
        // address cache, just like a direct struct global. Passing the pointer
        // itself still requires scalar value reuse across the guard.
        let scalar_uses = super::callee_saved::rewrite_structured_expression(
            expression,
            &mut |expression| match expression {
                Expression::Member { base, .. } if matches!(base.as_ref(), Expression::Variable(name) if name == global) => {
                    Some(Expression::IntegerLiteral(0))
                }
                _ => None,
            },
        );
        expression_reads_name(&scalar_uses, global)
    };
    match statement {
        Statement::Expression(expression) => reads_value(expression),
        Statement::Store { target, value }
            if is_independently_reloaded_narrow_increment(target, value, global, global_type) =>
        {
            false
        }
        Statement::Store { value, .. } => reads_value(value),
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
    #[test]
    fn struct_pointer_members_use_the_address_owner_but_pointer_arguments_do_not() {
        let ty = Type::StructPointer { element_size: 12 };
        let member = Expression::Member {
            base: Box::new(variable("state")),
            offset: 4,
            member_type: Type::UnsignedInt,
            index_stride: None,
        };
        let member_call = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![member.clone()],
        });
        assert!(!guarded_body_read_needs_value_reuse(
            &member_call,
            "state",
            ty
        ));
        let pointer_call = Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![member, variable("state")],
        });
        assert!(guarded_body_read_needs_value_reuse(
            &pointer_call,
            "state",
            ty
        ));
    }
}
