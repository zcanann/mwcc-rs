//! Storage-preserving assignments to address-taken narrow locals.

#[allow(unused_imports)]
use super::*;

pub(super) fn preserves_narrow_storage(
    name: &str,
    value: &Expression,
    value_type: Type,
) -> bool {
    let width = value_type.width();
    if width >= 32 || matches!(value_type, Type::Float | Type::Double) {
        return false;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = value
    else {
        return false;
    };
    let expected_mask = (1i64 << width) - 1;
    matches!(left.as_ref(), Expression::Variable(source) if source == name)
        && constant_value(right) == Some(expected_mask)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_full_width_byte_self_mask() {
        assert!(preserves_narrow_storage(
            "command",
            &Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("command".into())),
                right: Box::new(Expression::IntegerLiteral(255)),
            },
            Type::UnsignedChar,
        ));
    }

    #[test]
    fn rejects_a_mask_that_changes_storage_bits() {
        assert!(!preserves_narrow_storage(
            "command",
            &Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("command".into())),
                right: Box::new(Expression::IntegerLiteral(127)),
            },
            Type::UnsignedChar,
        ));
    }
}
