//! Pointer members handed from a guard into its first indirect call.
//!
//! Testing `object->callback` establishes the callback value on the taken edge.
//! When the first guarded statement calls through that exact member, MWCC keeps
//! the value in r12 instead of loading it again. This plan supplies semantic
//! identity and the ABI call-target home; the condition cache remains the
//! authority for instruction-level liveness and mutation barriers.

use super::*;

pub(super) struct Plan<'a> {
    pub(super) member: Expression,
    pub(super) followup: &'a Expression,
    pub(super) preferred_register: u8,
}

pub(super) fn plan<'a>(
    condition: &Expression,
    then_body: &'a [Statement],
) -> Option<Plan<'a>> {
    let member = tested_pointer_member(condition)?;
    let Some(Statement::Expression(call @ Expression::CallThrough { target, .. })) =
        then_body.first()
    else {
        return None;
    };
    crate::condition_member_cache::same_member(member, target).then(|| Plan {
        member: member.clone(),
        followup: call,
        preferred_register: 12,
    })
}

pub(super) fn plan_either_arm<'a>(
    condition: &Expression,
    then_body: &'a [Statement],
    else_body: &'a [Statement],
) -> Option<Plan<'a>> {
    plan(condition, then_body).or_else(|| plan(condition, else_body))
}

fn tested_pointer_member(expression: &Expression) -> Option<&Expression> {
    match expression {
        member @ Expression::Member {
            member_type: Type::Pointer(_) | Type::StructPointer { .. },
            index_stride: None,
            ..
        } => Some(member),
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if crate::analysis::constant_value(right) == Some(0)
            && matches!(left.as_ref(), Expression::Member { .. }) =>
        {
            tested_pointer_member(left)
        }
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if crate::analysis::constant_value(left) == Some(0)
            && matches!(right.as_ref(), Expression::Member { .. }) =>
        {
            tested_pointer_member(right)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callback() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 40,
            member_type: Type::Pointer(Pointee::Int),
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_a_tested_member_called_on_the_taken_edge() {
        let body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(callback()),
            arguments: vec![Expression::Variable("object".into())],
        })];
        let plan = plan(&callback(), &body).expect("guarded callback handoff");
        assert_eq!(plan.preferred_register, 12);
        assert!(crate::condition_member_cache::same_member(
            &plan.member,
            match plan.followup {
                Expression::CallThrough { target, .. } => target,
                _ => unreachable!(),
            },
        ));
    }

    #[test]
    fn rejects_a_call_through_a_different_member() {
        let mut other = callback();
        let Expression::Member { offset, .. } = &mut other else {
            unreachable!()
        };
        *offset = 44;
        let body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(other),
            arguments: Vec::new(),
        })];
        assert!(plan(&callback(), &body).is_none());
    }

    #[test]
    fn recognizes_the_same_member_called_at_the_else_entry() {
        let else_body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(callback()),
            arguments: vec![Expression::Variable("object".into())],
        })];
        let plan = plan_either_arm(
            &Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(callback()),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            &[Statement::Return(Some(Expression::IntegerLiteral(1)))],
            &else_body,
        )
        .expect("else-entry callback handoff");
        let Statement::Expression(expected) = &else_body[0] else {
            unreachable!()
        };
        assert!(std::ptr::eq(plan.followup, expected));
    }
}
