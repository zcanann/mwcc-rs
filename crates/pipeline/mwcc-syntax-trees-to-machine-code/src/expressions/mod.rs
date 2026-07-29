//! Core integer expression evaluation and operand placement.
//!
//! Split by family (fire 528); behavior-identical to the former single expressions.rs.

mod arithmetic;
mod aggregate_member_arithmetic;
mod aggregate_member_copy;
mod biased_scaled_member_sum;
mod bit_field_stores;
mod bit_fields;
mod call_argument_schedules;
mod call_argument_types;
mod call_indexed_member;
mod calls;
mod constructed_new;
mod constructor_initializers;
mod driver;
mod frame_aggregate_copy;
mod frame_matrix;
mod fixed_bank_store_schedule;
mod function_address;
mod global_array_decay;
mod global_array_index;
mod globals;
mod implicit_narrow_store;
mod index_operand;
mod indexed_rmw;
mod members;
mod narrow_compound;
mod operands;
mod passes;
mod pointer_alignment;
mod pointer_spans;
mod pointers;
mod post_step;
mod packed_shift_mask;
mod punned_displacement;
mod shifted_member_mask;
mod stores;
mod strings;
mod xnor_feedback_update;

pub(crate) use members::embedded_member_address_base;
pub(crate) use packed_shift_mask::is_shallow_packed_shift_mask_expression;
#[allow(unused_imports)]
pub(crate) use passes::*;

pub(crate) use crate::analysis::*;
pub(crate) use crate::generator::*;
pub(crate) use crate::operands::*;
pub(crate) use mwcc_core::{Compilation, Diagnostic};
pub(crate) use mwcc_machine_code::{Instruction, RelocationKind};
pub(crate) use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
pub(crate) use mwcc_target::Eabi;
pub(crate) use mwcc_versions::{
    BitFieldLoadPlacement, FunctionAddressStoreStyle, GlobalAddressing,
    GlobalArrayDecayStoreStyle, MaterializationCopyStyle,
};
