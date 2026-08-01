//! Register cleanliness after structured local assignments.
//!
//! A narrow local keeps its source-language storage type, but an unsigned
//! byte/halfword member load already has zeroes in every upper register bit.
//! Consumers can use that current value as a full-width unsigned register
//! without emitting another `clrlwi`.

use mwcc_syntax_trees::{Expression, Type};
use std::collections::HashMap;

pub(super) fn assigned_register_width(
    declared_type: Type,
    value: &Expression,
    call_return_types: &HashMap<String, Type>,
) -> u8 {
    let assigned_call_width = match value {
        Expression::Call { name, .. } => call_return_types.get(name).map(|ty| ty.width()),
        Expression::Comma { right, .. } => {
            return assigned_register_width(declared_type, right, call_return_types);
        }
        _ => None,
    };
    if assigned_call_width.is_some_and(|width| width < declared_type.width() && width < 32) {
        return assigned_call_width.expect("narrow call width was checked");
    }
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
            assigned_register_width(
                Type::UnsignedShort,
                &member(Type::UnsignedShort),
                &HashMap::new(),
            ),
            32
        );
        assert_eq!(
            assigned_register_width(
                Type::UnsignedChar,
                &member(Type::UnsignedChar),
                &HashMap::new(),
            ),
            32
        );
    }

    #[test]
    fn unrelated_assignments_keep_the_declared_width() {
        assert_eq!(
            assigned_register_width(Type::UnsignedShort, &member(Type::Short), &HashMap::new()),
            16
        );
        assert_eq!(
            assigned_register_width(
                Type::UnsignedShort,
                &Expression::Variable("source".into()),
                &HashMap::new(),
            ),
            16
        );
    }

    #[test]
    fn promoted_call_assignments_retain_the_result_source_width() {
        let returns = HashMap::from([("short_call".to_owned(), Type::Short)]);
        let call = Expression::Call {
            name: "short_call".into(),
            arguments: Vec::new(),
        };
        assert_eq!(assigned_register_width(Type::Int, &call, &returns), 16);
    }
}
