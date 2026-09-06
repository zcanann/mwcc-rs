//! Source-call spellings that MWCC lowers as target instructions.
//!
//! Intrinsics retain call-shaped syntax, but they do not branch, clobber LR, or
//! contribute an external symbol. Keep their identity and arity in one place so
//! frame planning, symbol traversal, and expression lowering agree.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Intrinsic {
    FloatAbsolute,
    IntegerAbsolute,
    Synchronize,
    InstructionSynchronize,
    EnforceInOrderIo,
}

pub(crate) fn classify(name: &str, argument_count: usize) -> Option<Intrinsic> {
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
        Some(Intrinsic::FloatAbsolute | Intrinsic::IntegerAbsolute)
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
    classify(name, argument_count) == Some(Intrinsic::IntegerAbsolute)
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
}
