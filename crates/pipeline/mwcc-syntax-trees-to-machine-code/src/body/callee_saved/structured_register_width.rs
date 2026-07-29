//! Register cleanliness after structured local assignments.
//!
//! A narrow local keeps its source-language storage type, but an unsigned
//! byte/halfword member load already has zeroes in every upper register bit.
//! Consumers can use that current value as a full-width unsigned register
//! without emitting another `clrlwi`.

use mwcc_syntax_trees::{Expression, Type};

pub(super) fn assigned_register_width(declared_type: Type, value: &Expression) -> u8 {
    let clean_unsigned_load = matches!(
        (declared_type, value),
        (
            Type::UnsignedChar,
            Expression::Member {
                member_type: Type::UnsignedChar,
                ..
            }
        ) | (
            Type::UnsignedShort,
            Expression::Member {
                member_type: Type::UnsignedShort,
                ..
            }
        )
    );
    if clean_unsigned_load {
        32
    } else {
        declared_type.width()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset: 4,
            member_type,
            index_stride: None,
        }
    }

    #[test]
    fn unsigned_member_loads_are_clean_register_values() {
        assert_eq!(
            assigned_register_width(Type::UnsignedShort, &member(Type::UnsignedShort)),
            32
        );
        assert_eq!(
            assigned_register_width(Type::UnsignedChar, &member(Type::UnsignedChar)),
            32
        );
    }

    #[test]
    fn unrelated_assignments_keep_the_declared_width() {
        assert_eq!(
            assigned_register_width(Type::UnsignedShort, &member(Type::Short)),
            16
        );
        assert_eq!(
            assigned_register_width(
                Type::UnsignedShort,
                &Expression::Variable("source".into())
            ),
            16
        );
    }
}
