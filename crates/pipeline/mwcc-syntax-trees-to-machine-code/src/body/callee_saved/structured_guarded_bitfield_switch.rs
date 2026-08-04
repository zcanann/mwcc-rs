//! True-edge reuse for a bitfield guard immediately followed by its switch.
//!
//! A bitfield condition already leaves its extracted value in r0 while setting
//! CR0 with a record-form rotate/mask. When the guarded body's first statement
//! dispatches on the identical field, MWCC feeds that value straight into the
//! range check instead of loading and extracting the storage again.

#[allow(unused_imports)]
use super::*;

pub(super) fn recognize(
    condition: &Expression,
    then_body: &[Statement],
) -> Option<Expression> {
    if !matches!(condition, Expression::BitFieldRead { .. }) {
        return None;
    }
    let Some(Statement::Switch {
        scrutinee, arms, ..
    }) = then_body.first()
    else {
        return None;
    };
    (super::structured_switch_lowering::is_dense_structured_switch(arms)
        && same_field(condition, scrutinee))
    .then(|| condition.clone())
}

pub(super) fn consume(cache: &mut Option<Expression>, scrutinee: &Expression) -> bool {
    let reusable = cache
        .as_ref()
        .is_some_and(|value| same_field(value, scrutinee));
    if reusable {
        *cache = None;
    }
    reusable
}

fn same_field(left: &Expression, right: &Expression) -> bool {
    let Expression::BitFieldRead {
        extracted: left_extracted,
        promoted_type: left_type,
        storage: left_storage,
        shift: left_shift,
        width: left_width,
    } = left
    else {
        return false;
    };
    let Expression::BitFieldRead {
        extracted: right_extracted,
        promoted_type: right_type,
        storage: right_storage,
        shift: right_shift,
        width: right_width,
    } = right
    else {
        return false;
    };
    left_type == right_type
        && left_shift == right_shift
        && left_width == right_width
        && crate::analysis::structurally_equal(left_storage, right_storage)
        && crate::analysis::structurally_equal(left_extracted, right_extracted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ArmBody, SwitchArm};

    fn field(shift: u8) -> Expression {
        let storage = Expression::Member {
            base: Box::new(Expression::Variable("record".into())),
            offset: 1,
            member_type: Type::UnsignedChar,
            index_stride: None,
        };
        Expression::BitFieldRead {
            extracted: Box::new(storage.clone()),
            promoted_type: Type::Int,
            storage: Box::new(storage),
            shift,
            width: 4,
        }
    }

    fn dense_switch(scrutinee: Expression) -> Statement {
        Statement::Switch {
            scrutinee,
            arms: [1, 2, 3, 5, 6, 7]
                .into_iter()
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: false,
                })
                .collect(),
            default: None,
        }
    }

    #[test]
    fn retains_an_identical_guard_value_for_the_first_dense_switch() {
        let condition = field(4);
        let retained = recognize(&condition, &[dense_switch(condition.clone())])
            .expect("identical bitfield guard should be reused");
        let mut cache = Some(retained);
        assert!(consume(&mut cache, &condition));
        assert!(cache.is_none());
    }

    #[test]
    fn rejects_a_different_field_or_intervening_statement() {
        let condition = field(4);
        assert!(recognize(&condition, &[dense_switch(field(0))]).is_none());
        assert!(recognize(
            &condition,
            &[
                Statement::Expression(Expression::IntegerLiteral(0)),
                dense_switch(condition.clone()),
            ],
        )
        .is_none());
    }
}
