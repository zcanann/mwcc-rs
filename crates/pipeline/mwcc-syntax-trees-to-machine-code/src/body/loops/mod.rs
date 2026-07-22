//! Loop-family recognizers, split by theme from a single 2795-line module.

#[allow(unused_imports)]
use super::*;

mod ascii_hash;
mod ascii_upper;
mod count_register;
mod counters;
mod fill_copy;
mod fixed_port_zero_fill;
mod guarded_byte_copy;
mod indexed_calls;
mod inlined_byte_append;
pub(crate) mod policy;
mod poll_search;
mod search_guard_chain;
mod status_indexed_call;
mod virtual_scan;
mod walker;
mod xnor_feedback;

/// Signedness of a byte pointer's element. Plain `char *` has already been
/// resolved by the frontend to one of these two types for the selected mwcc
/// build, so loop lowering should follow the type instead of consulting the
/// build number again.
fn byte_pointer_signedness(value_type: Type) -> Option<bool> {
    match value_type {
        Type::Pointer(Pointee::Char) => Some(true),
        Type::Pointer(Pointee::UnsignedChar) => Some(false),
        _ => None,
    }
}

/// Test a byte loaded by a pointer-walk loop for zero. A signed byte uses
/// mwcc's `extsb.` fusion; an unsigned byte is already promoted by `lbz` and
/// uses the unsigned compare-immediate form.
fn byte_truth_test(register: u8, signed: bool) -> Instruction {
    if signed {
        Instruction::ExtendSignByteRecord { a: 0, s: register }
    } else {
        Instruction::CompareLogicalWordImmediate {
            a: register,
            immediate: 0,
        }
    }
}
