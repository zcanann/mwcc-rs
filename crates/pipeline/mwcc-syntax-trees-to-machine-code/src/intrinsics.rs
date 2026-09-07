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
