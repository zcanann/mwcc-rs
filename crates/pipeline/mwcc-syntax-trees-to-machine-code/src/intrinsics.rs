//! Source-call spellings that MWCC lowers as target instructions.
//!
//! Intrinsics retain call-shaped syntax, but they do not branch, clobber LR, or
//! contribute an external symbol. Keep their identity and arity in one place so
//! frame planning, symbol traversal, and expression lowering agree.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Intrinsic {
    FloatAbsolute,
    IntegerAbsolute,
}

pub(crate) fn classify(name: &str, argument_count: usize) -> Option<Intrinsic> {
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
    fn classifies_only_measured_unary_spellings() {
        assert_eq!(classify("__fabs", 1), Some(Intrinsic::FloatAbsolute));
        assert_eq!(classify("__abs", 1), Some(Intrinsic::IntegerAbsolute));
        assert_eq!(classify("abs", 1), None);
        assert_eq!(classify("__abs", 0), None);
        assert_eq!(classify("__abs", 2), None);
    }
}
