//! Core integer expression evaluation and operand placement.
//!
//! Split by family (fire 528); behavior-identical to the former single expressions.rs.

mod arithmetic;
mod adjacent_fixed_bank_stores;
mod affine_member_pointer_store;
mod aggregate_member_arithmetic;
mod aggregate_member_copy;
mod aggregate_member_copy_loop;
mod biased_scaled_member_sum;
mod by_value_aggregate_arguments;
mod bit_field_stores;
mod bit_fields;
mod call_argument_schedules;
mod call_argument_types;
mod call_indexed_member;
mod calls;
mod constructed_new;
mod constructor_initializers;
mod computed_index_subscript;
mod driver;
mod frame_aggregate_copy;
mod frame_array_indexed_load;
mod frame_array_indexed_store;
mod frame_subobject;
mod frame_matrix;
mod fixed_bank_store_schedule;
mod function_address;
mod global_array_decay;
mod global_array_element_address;
mod global_array_index;
mod global_pointer_array_member_load;
mod global_pointer_table_entry;
mod global_pointer_table_member_store;
mod globals;
mod global_member_pointer_indexed_store;
mod implicit_narrow_store;
mod integer_abs_pair_binary;
mod index_operand;
mod indexed_call_result_store;
mod indexed_rmw;
mod linkage_first_fixed_bank_region;
mod linkage_first_fixed_bank_self_copy;
mod materialized_bitand_constant;
mod member_array_constant_store;
mod member_pointer_constant_indexed_store;
mod member_indexed_global_array_store;
mod retained_global_pointer_store;
mod scaled_integer_call_narrow_store;
mod spr_instruction_encoding;
mod members;
mod narrow_compound;
mod nested_global_pointer_float_store;
mod nested_global_member_pointer_store;
mod nested_global_member_pointer_variable_store;
mod nested_member_array_load;
mod nested_member_array_store;
mod nested_pointer_table;
mod non_power_struct_member_store;
mod operands;
mod passes;
mod pointer_alignment;
mod pointer_member_scaled_offset;
mod pointer_spans;
mod pointers;
mod post_step;
mod packed_shift_mask;
mod punned_displacement;
mod shifted_member_mask;
mod stores;
mod strings;
mod wide_call_arguments;
mod wide_pointer_mask_store;
mod xnor_feedback_update;

pub(crate) use members::embedded_member_address_base;
pub(crate) use call_argument_types::source_parameter_type;
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
