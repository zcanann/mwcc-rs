//! Prototype-directed classification of call arguments.
//!
//! Source expressions do not by themselves determine an ABI register class: an
//! integer constant passed to a `float` parameter is folded to a floating pool
//! load, while a nonconstant integer needs a real conversion sequence. Keep that
//! decision separate from the call scheduler and register marshaling.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CallArgumentPlacement {
    General,
    Floating {
        parameter_type: Type,
        folded_integer: Option<f64>,
        convert_integer: bool,
    },
}

/// Stack slot for an integer-class argument after r3..r10 are exhausted.
///
/// The first overflow word occupies the caller parameter area at `8(r1)`;
/// each subsequent word advances by four bytes.
pub(super) fn outgoing_general_stack_offset(next_register: u8) -> Option<i16> {
    (next_register > Eabi::LAST_GENERAL_ARGUMENT)
        .then(|| 8 + i16::from(next_register - Eabi::LAST_GENERAL_ARGUMENT - 1) * 4)
}

pub(super) fn classify_call_argument(
    parameter_type: Option<Type>,
    argument_is_float: bool,
    integer_constant: Option<i64>,
) -> Compilation<CallArgumentPlacement> {
    match parameter_type {
        Some(parameter_type @ (Type::Float | Type::Double)) => {
            if argument_is_float {
                Ok(CallArgumentPlacement::Floating {
                    parameter_type,
                    folded_integer: None,
                    convert_integer: false,
                })
            } else if let Some(value) = integer_constant {
                Ok(CallArgumentPlacement::Floating {
                    parameter_type,
                    folded_integer: Some(value as f64),
                    convert_integer: false,
                })
            } else {
                Ok(CallArgumentPlacement::Floating {
                    parameter_type,
                    folded_integer: None,
                    convert_integer: true,
                })
            }
        }
        Some(_) if argument_is_float => Err(Diagnostic::error(
            "a floating call argument needs float->int conversion (roadmap)",
        )),
        Some(_) => Ok(CallArgumentPlacement::General),
        None if argument_is_float => Ok(CallArgumentPlacement::Floating {
            // With no prototype, retain the expression-driven historical
            // default. Float literals and float values use single precision.
            parameter_type: Type::Float,
            folded_integer: None,
            convert_integer: false,
        }),
        None => Ok(CallArgumentPlacement::General),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_an_integer_constant_for_a_float_parameter() {
        assert_eq!(
            classify_call_argument(Some(Type::Float), false, Some(1)).unwrap(),
            CallArgumentPlacement::Floating {
                parameter_type: Type::Float,
                folded_integer: Some(1.0),
                convert_integer: false,
            }
        );
    }

    #[test]
    fn classifies_a_nonconstant_integer_for_runtime_conversion() {
        assert_eq!(
            classify_call_argument(Some(Type::Float), false, None).unwrap(),
            CallArgumentPlacement::Floating {
                parameter_type: Type::Float,
                folded_integer: None,
                convert_integer: true,
            }
        );
    }

    #[test]
    fn assigns_general_overflow_words_above_the_linkage_area() {
        assert_eq!(
            outgoing_general_stack_offset(Eabi::LAST_GENERAL_ARGUMENT),
            None
        );
        assert_eq!(
            outgoing_general_stack_offset(Eabi::LAST_GENERAL_ARGUMENT + 1),
            Some(8)
        );
        assert_eq!(
            outgoing_general_stack_offset(Eabi::LAST_GENERAL_ARGUMENT + 2),
            Some(12)
        );
    }
}
