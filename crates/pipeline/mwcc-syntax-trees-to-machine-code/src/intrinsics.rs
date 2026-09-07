//! Source-call spellings that MWCC lowers as target instructions.
//!
//! Intrinsics retain call-shaped syntax, but they do not branch, clobber LR, or
//! contribute an external symbol. Keep their identity and arity in one place so
//! frame planning, symbol traversal, and expression lowering agree.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Intrinsic {
    FloatAbsolute,
    IntegerAbsolute,
    RotateLeftWordInsert,
    Synchronize,
    InstructionSynchronize,
    EnforceInOrderIo,
}

/// Validated operands shared by general intrinsic selection and body schedules.
pub(crate) struct RotateInsert<'a> {
    pub(crate) initial: &'a mwcc_syntax_trees::Expression,
    pub(crate) source: &'a mwcc_syntax_trees::Expression,
    pub(crate) shift: u8,
    pub(crate) begin: u8,
    pub(crate) end: u8,
}

pub(crate) fn rotate_insert(
    arguments: &[mwcc_syntax_trees::Expression],
) -> Option<RotateInsert<'_>> {
    let [initial, source, shift, begin, end] = arguments else {
        return None;
    };
    let immediate = |expression| {
        crate::analysis::constant_value(expression)
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value < 32)
    };
    Some(RotateInsert {
        initial,
        source,
        shift: immediate(shift)?,
        begin: immediate(begin)?,
        end: immediate(end)?,
    })
}

pub(crate) fn classify(name: &str, argument_count: usize) -> Option<Intrinsic> {
    if name == "__rlwimi" && argument_count == 5 {
        return Some(Intrinsic::RotateLeftWordInsert);
    }
    if argument_count == 0 {
        return match name {
            "__sync" => Some(Intrinsic::Synchronize),
            "__isync" => Some(Intrinsic::InstructionSynchronize),
            "__eieio" => Some(Intrinsic::EnforceInOrderIo),
            _ => None,
        };
    }
    if argument_count != 1 {
        return None;
    }
    match name {
        "__fabs" => Some(Intrinsic::FloatAbsolute),
        "__abs" => Some(Intrinsic::IntegerAbsolute),
        _ => None,
    }
}

pub(crate) fn is_intrinsic_call(name: &str, argument_count: usize) -> bool {
    classify(name, argument_count).is_some()
}

pub(crate) fn is_pure_intrinsic_call(name: &str, argument_count: usize) -> bool {
    matches!(
        classify(name, argument_count),
        Some(
            Intrinsic::FloatAbsolute | Intrinsic::IntegerAbsolute | Intrinsic::RotateLeftWordInsert
        )
    )
}

pub(crate) fn ordering_instruction(
    name: &str,
    argument_count: usize,
) -> Option<mwcc_machine_code::Instruction> {
    use mwcc_machine_code::Instruction;
    match classify(name, argument_count)? {
        Intrinsic::Synchronize => Some(Instruction::Synchronize),
        Intrinsic::InstructionSynchronize => Some(Instruction::InstructionSynchronize),
        Intrinsic::EnforceInOrderIo => Some(Instruction::EnforceInOrderIo),
        _ => None,
    }
}

pub(crate) fn is_float_intrinsic_call(name: &str, argument_count: usize) -> bool {
    classify(name, argument_count) == Some(Intrinsic::FloatAbsolute)
}

pub(crate) fn is_integer_intrinsic_call(name: &str, argument_count: usize) -> bool {
    matches!(
        classify(name, argument_count),
        Some(Intrinsic::IntegerAbsolute | Intrinsic::RotateLeftWordInsert)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_rotate_immediates_without_restricting_wrapping_masks() {
        use mwcc_syntax_trees::Expression;
        let mut arguments = vec![Expression::IntegerLiteral(0); 5];
        arguments[2] = Expression::IntegerLiteral(31);
        arguments[3] = Expression::IntegerLiteral(28);
        arguments[4] = Expression::IntegerLiteral(3);
        let insert = rotate_insert(&arguments).unwrap();
        assert_eq!((insert.shift, insert.begin, insert.end), (31, 28, 3));
        for index in 2..5 {
            for invalid in [
                Expression::IntegerLiteral(-1),
                Expression::IntegerLiteral(32),
                Expression::Variable("dynamic".into()),
            ] {
                let mut invalid_arguments = arguments.clone();
                invalid_arguments[index] = invalid;
                assert!(rotate_insert(&invalid_arguments).is_none());
            }
        }
        assert!(rotate_insert(&arguments[..4]).is_none());
    }

    #[test]
    fn ordering_intrinsics_are_effectful_without_calling() {
        for name in ["__sync", "__isync", "__eieio"] {
            let expression = mwcc_syntax_trees::Expression::Call {
                name: name.into(),
                arguments: Vec::new(),
            };
            assert!(!crate::analysis::expression_has_call(&expression));
            assert!(crate::analysis::expression_has_side_effect(&expression));
            assert!(ordering_instruction(name, 0).is_some());
            assert_eq!(classify(name, 1), None);
        }
    }

    #[test]
    fn classifies_only_measured_unary_spellings() {
        assert_eq!(classify("__fabs", 1), Some(Intrinsic::FloatAbsolute));
        assert_eq!(classify("__abs", 1), Some(Intrinsic::IntegerAbsolute));
        assert_eq!(classify("abs", 1), None);
        assert_eq!(classify("__abs", 0), None);
        assert_eq!(classify("__abs", 2), None);
    }

    #[test]
    fn rotate_insert_retains_effects_of_its_operands() {
        use mwcc_syntax_trees::Expression;
        let mut arguments = vec![Expression::IntegerLiteral(0); 5];
        let pure = Expression::Call {
            name: "__rlwimi".into(),
            arguments: arguments.clone(),
        };
        assert!(!crate::analysis::expression_has_call(&pure));
        assert!(!crate::analysis::expression_has_side_effect(&pure));
        arguments[1] = Expression::Call {
            name: "fetch".into(),
            arguments: Vec::new(),
        };
        let effectful = Expression::Call {
            name: "__rlwimi".into(),
            arguments,
        };
        assert!(crate::analysis::expression_has_call(&effectful));
        assert!(crate::analysis::expression_has_side_effect(&effectful));
        assert_eq!(classify("__rlwimi", 4), None);
        assert_eq!(classify("__rlwimi", 6), None);
    }
}
