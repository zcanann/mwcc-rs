//! Scheduling for a call result used as a struct-array index.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Prepare `owner->array[call(...)]` for the ordinary indexed-member load.
    ///
    /// The call must run before the array pointer is loaded because its result
    /// is the index in r3.  Loading the member-backed array into r4 afterward
    /// both preserves that result and matches mwcc's call/base/scale schedule.
    /// The owner's liveness across the call is left to the normal callee-saved
    /// allocator; this helper owns only the expression-local ordering.
    pub(crate) fn try_prepare_call_indexed_member(
        &mut self,
        array: &Expression,
        index: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !matches!(index, Expression::Call { .. }) {
            return Ok(None);
        }
        let Expression::Member {
            base,
            member_type: Type::Pointer(_) | Type::StructPointer { .. },
            index_stride: None,
            ..
        } = array
        else {
            return Ok(None);
        };
        if self.general_register_of_leaf(base).is_err() {
            return Ok(None);
        }

        let index_register = Eabi::general_result().number;
        let array_register = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.evaluate_general(index, index_register)?;
        self.evaluate_general(array, array_register)?;
        Ok(Some((array_register, index_register)))
    }
}
