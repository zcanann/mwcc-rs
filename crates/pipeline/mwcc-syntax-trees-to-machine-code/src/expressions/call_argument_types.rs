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
    WideGeneral {
        parameter_type: Type,
    },
    ConvertFloatingToGeneral {
        parameter_type: Type,
    },
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
        Some(parameter_type @ (Type::LongLong | Type::UnsignedLongLong)) => {
            Ok(CallArgumentPlacement::WideGeneral { parameter_type })
        }
        Some(parameter_type) if argument_is_float => {
            Ok(CallArgumentPlacement::ConvertFloatingToGeneral { parameter_type })
        }
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

/// Convert a word-sized integer register to the declared narrow parameter ABI
/// value. MWCC writes the converted value directly to the argument register;
/// the source can remain in a saved register when it is still live.
pub(super) fn narrow_general_argument(
    parameter_type: Type,
    argument_register: u8,
    source_register: u8,
) -> Option<Instruction> {
    match parameter_type {
        Type::Char => Some(Instruction::ExtendSignByte {
            a: argument_register,
            s: source_register,
        }),
        Type::Short => Some(Instruction::ExtendSignHalfword {
            a: argument_register,
            s: source_register,
        }),
        Type::UnsignedChar => Some(Instruction::ClearLeftImmediate {
            a: argument_register,
            s: source_register,
            clear: 24,
        }),
        Type::UnsignedShort => Some(Instruction::ClearLeftImmediate {
            a: argument_register,
            s: source_register,
            clear: 16,
        }),
        _ => None,
    }
}

/// A scalar local assignment used directly as an argument leaves its result in
/// that local's allocator-owned home before prototype conversion.
pub(super) fn assigned_general_name(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, .. } = expression else {
        return None;
    };
    let Expression::Variable(name) = target.as_ref() else {
        return None;
    };
    Some(name)
}

/// Map an ABI argument index back to the source prototype.
///
/// Aggregate-returning calls prepend a hidden result address which is absent
/// from the source parameter list. Keeping the shift here prevents each call
/// schedule from independently (and inconsistently) classifying the receiver
/// or first explicit argument against the wrong prototype slot.
pub(crate) fn source_parameter_type(
    parameter_types: Option<&[Type]>,
    returns_aggregate: bool,
    abi_argument_count: usize,
    abi_index: usize,
) -> Option<Type> {
    let parameter_types = parameter_types?;
    let hidden_result =
        returns_aggregate && abi_argument_count == parameter_types.len().checked_add(1)?;
    let source_index = abi_index.checked_sub(usize::from(hidden_result))?;
    parameter_types.get(source_index).copied()
}

#[derive(Clone, Copy)]
pub(super) enum AggregateReferenceSource<'a> {
    /// `*p` passed by reference: the address expression is already `p`.
    Address(&'a Expression),
    /// A struct-valued lvalue such as `object.member`: form `&lvalue`.
    Lvalue(&'a Expression),
}

/// A C++ reference parameter is represented as a struct pointer in the compact
/// ABI types. Recover the source address rather than trying to scalar-load the
/// aggregate value.
pub(super) fn aggregate_reference_source<'a>(
    argument: &'a Expression,
    parameter_type: Option<Type>,
    source_size: impl FnOnce(&Expression) -> Option<u32>,
) -> Option<AggregateReferenceSource<'a>> {
    let Some(Type::StructPointer { element_size }) = parameter_type else {
        return None;
    };
    let compatible = |size| element_size == 0 || size == element_size;
    match argument {
        Expression::Dereference { pointer } => {
            let source_size = source_size(pointer)?;
            compatible(source_size).then_some(AggregateReferenceSource::Address(pointer))
        }
        Expression::Member {
            member_type: Type::Struct { size, .. },
            ..
        } if compatible(*size) => Some(AggregateReferenceSource::Lvalue(argument)),
        _ => None,
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
    fn classifies_a_float_for_runtime_integer_conversion() {
        assert_eq!(
            classify_call_argument(Some(Type::Int), true, None).unwrap(),
            CallArgumentPlacement::ConvertFloatingToGeneral {
                parameter_type: Type::Int,
            }
        );
    }

    #[test]
    fn classifies_wide_integer_formals_as_register_pairs() {
        assert_eq!(
            classify_call_argument(Some(Type::LongLong), false, None).unwrap(),
            CallArgumentPlacement::WideGeneral {
                parameter_type: Type::LongLong,
            }
        );
        assert_eq!(
            classify_call_argument(Some(Type::UnsignedLongLong), false, Some(7)).unwrap(),
            CallArgumentPlacement::WideGeneral {
                parameter_type: Type::UnsignedLongLong,
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

    #[test]
    fn narrows_a_saved_word_directly_into_the_argument_register() {
        assert!(matches!(
            narrow_general_argument(Type::Short, 3, 31),
            Some(Instruction::ExtendSignHalfword { a: 3, s: 31 })
        ));
        assert!(matches!(
            narrow_general_argument(Type::UnsignedChar, 4, 30),
            Some(Instruction::ClearLeftImmediate {
                a: 4,
                s: 30,
                clear: 24
            })
        ));
        assert!(narrow_general_argument(Type::Int, 3, 31).is_none());
    }

    #[test]
    fn recognizes_an_assignment_argument_with_a_named_home() {
        let assignment = Expression::Assign {
            target: Box::new(Expression::Variable("saved".into())),
            value: Box::new(Expression::IntegerLiteral(4)),
        };
        assert_eq!(assigned_general_name(&assignment), Some("saved"));
        assert!(assigned_general_name(&Expression::IntegerLiteral(4)).is_none());
    }

    #[test]
    fn shifts_source_types_past_an_aggregate_hidden_result() {
        let types = [Type::StructPointer { element_size: 12 }, Type::Float];

        assert_eq!(source_parameter_type(Some(&types), true, 3, 0), None);
        assert_eq!(
            source_parameter_type(Some(&types), true, 3, 1),
            Some(types[0])
        );
        assert_eq!(
            source_parameter_type(Some(&types), true, 3, 2),
            Some(Type::Float)
        );
    }

    #[test]
    fn passes_a_dereferenced_aggregate_reference_as_its_address() {
        let pointer = Expression::Variable("object".into());
        let argument = Expression::Dereference {
            pointer: Box::new(pointer.clone()),
        };

        assert!(matches!(
            aggregate_reference_source(
                &argument,
                Some(Type::StructPointer { element_size: 108 }),
                |_| Some(108),
            ),
            Some(AggregateReferenceSource::Address(Expression::Variable(name)))
                if name == "object"
        ));
        assert!(aggregate_reference_source(
            &argument,
            Some(Type::StructPointer { element_size: 64 }),
            |_| Some(108),
        )
        .is_none());
        assert!(aggregate_reference_source(
            &argument,
            Some(Type::StructPointer { element_size: 0 }),
            |_| Some(108),
        )
        .is_some());
    }

    #[test]
    fn forms_the_address_of_a_struct_member_reference_argument() {
        let member = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 68,
            member_type: Type::Struct { size: 12, align: 4 },
            index_stride: None,
        };

        assert!(matches!(
            aggregate_reference_source(
                &member,
                Some(Type::StructPointer { element_size: 0 }),
                |_| None,
            ),
            Some(AggregateReferenceSource::Lvalue(Expression::Member {
                offset: 68,
                ..
            }))
        ));
    }
}
