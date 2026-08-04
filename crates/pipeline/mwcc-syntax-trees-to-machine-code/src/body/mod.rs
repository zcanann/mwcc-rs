//! Function-level emission: parameters, body, guards, and the return tail.
//!
//! Split by family (fire 525); behavior-identical to the former single body.rs.

mod call_prologue;
mod guarded_member_call_entry_schedule;
mod callback_publication_schedule;
mod call_result_if_else;
mod call_result_member_callback_guard;
mod leading_bitfield_clear_call;
mod leading_float_update_clamp;
mod terminal_float_update;
mod member_acceleration_clamp;
mod member_float_friction_select;
mod leading_member_store_call;
mod leaf_tail_append_schedule;
mod leaf_singly_linked_unlink_schedule;
mod release_to_global_manager;
mod bounded_global_ring_remove;
mod leading_shared_zero_bitfield_guard;
mod linker_table_initialization;
mod linkage_first_address_argument_schedule;
mod inlined_sign_store_schedule;
mod inlined_context_clear_schedule;
mod inlined_acceleration_select_schedule;
mod inlined_symmetric_float_clamp_schedule;
mod joystick_count_schedule;
mod grab_mash_schedule;
mod mixed_member_zero_reset_schedule;
mod one_word_aggregate;
mod return_store_schedule;
mod canonical_boolean;
mod symmetric_float_clamp;
mod symmetric_sum_clamp_schedule;
mod structured_float_or_schedule;
mod structured_float_clamp_scale_schedule;
mod symmetric_float_decay;
mod symmetric_float_decay_return;
mod sign_selected_member_store;
mod aggregate_return_forwarder;
mod aggregate_local_return;
mod aggregate_parameter_forwarder;
mod aggregate_return_temporaries;
mod ascii_pointer_compare;
mod assertion_expression;
mod assigned_pointer_alias;
mod bounded_member_cursor;
mod bounded_member_assignment;
mod bounded_global_array_search;
mod bounded_acceleration_schedule;
mod bounded_vector_reciprocal_schedule;
mod adjacent_fighter_nudge_schedule;
mod guarded_item_charge_schedule;
mod damage_vector_schedule;
mod dual_status_switch_schedule;
mod retained_item_ratio_schedule;
mod callee_saved;
pub(crate) use callee_saved::owns_unreferenced_forwarding_branch_cleanup;
pub(crate) use callee_saved::branches_enter_float_restores;
pub(crate) use callee_saved::restores_fprs_before_gpr_helper_setup;
mod comma_operator;
mod coalescing_free_list_insert;
mod condition_linkage;
mod conditional_float_call_arguments;
mod conditional_float_requantize;
mod conditional_friction_select;
mod conditional_integer_call_arguments;
mod conditional_global_array_publication;
mod conditional;
mod conditional_member_copy;
mod constructor_pod_initialization_schedule;
mod conditional_member_select_tail;
mod control_block_unique_copy;
mod cxx_global_startup;
mod dispatchers;
mod display_list_base;
mod display_list_coveredge;
mod display_list_padding;
mod display_list_packets;
mod display_list_packet_runs;
mod display_list_framebuffer_setup;
mod device_registration_event_switch;
mod doubly_linked_list_extract;
mod dense_virtual_switch_dispatch;
mod global_doubly_linked_remove;
mod global_doubly_linked_append_trace;
mod global_status_snapshot_access;
mod global_pointer_table_link_search;
mod memory_access_transaction;
mod memory_map_validation;
mod extended_register_access;
mod support_file_request;
mod driver;
mod enum_remap_member_update;
mod endian_probe;
mod endian_stack_pack;
mod endian_stack_unpack;
mod cached_member_guard;
mod chunked_callback_read;
mod expression_statement;
mod float_store_fill;
mod forward_pointer_global_copy;
mod linkage_first_condition_member_reuse;
mod linkage_first_disjoint_scratch_frame;
mod linkage_first_guarded_global_member_base;
mod linkage_first_pointer_publication;
mod linkage_first_post_asm_variadic_store_schedule;
mod formatter_buffer_copy_schedule;
mod formatter_character_schedule;
mod formatter_position_schedule;
mod frame_row_string_append_schedule;
mod guarded_formatter_member_cache_schedule;
mod guarded_integer_constant_reuse;
mod float_friction_select;
mod float_call_guard_return;
mod fp_register_transfer;
mod fp_register_access;
mod paired_single_register_access;
mod float_octant_table_dispatch;
mod forwarded_member_initialization_schedule;
mod friction_limited_acceleration_clamp;
mod fixed_address_object_flush;
mod fixed_bank_transformed_load;
mod fixed_port_bitfield;
mod fixed_port_global_replay;
mod fixed_port_indexed_bitfield;
mod fixed_port_matrix_packets;
mod fixed_port_mask_accumulation;
mod fixed_port_order_switch;
mod fixed_port_packet_accumulator;
mod fixed_port_scale_switch;
mod fixed_port_replay_update;
mod frame_vector_accumulation_schedule;
mod global_aggregate_constant_initialization;
mod global_struct_binary_search_schedule;
mod hierarchy_push_pop_schedule;
mod recorded_boolean_zero_test;
mod guards_ifs;
mod global_struct_member_search;
mod global_bitfield_dirty_update;
mod ground_knockback_projection_schedule;
mod guarded_aggregate_update;
mod guarded_float_table_index;
mod guarded_global_callback;
mod guarded_global_rmw;
mod guarded_member_decrement_if_else;
mod if_else;
mod indirect_call;
mod inlined_guarded_aggregate_update;
mod inlined_callback_open;
mod inlined_doubly_linked_list_transfer;
mod inlined_local_select;
mod inlined_quadratic_float_map_loop;
mod inlined_object_make;
mod ladders;
mod legacy_constant_store;
mod leading_store_guard;
mod leaf_shared_constant_return;
mod leading_store_trailing_if;
mod leading_store_guarded_call;
mod local_select;
mod local_member_call_dispatch;
mod loop_normalization;
mod long_long_initialize;
mod long_long_serial_fold;
mod long_long_support;
mod long_long_wait;
mod loops;
mod member_copy_call;
mod member_float_normalize;
mod member_initialization;
mod member_linefeed;
mod member_rect_control;
mod member_equality_range_schedule;
mod member_store_forwarding;
mod member_tab;
mod member_store_fill;
mod materialized_float_assignment;
mod materialized_store_locals;
mod masked_word_store_switch;
mod masked_transfer_command_switch;
mod mixed_scalar_initialization_schedule;
mod nested_global_indirect_call;
mod passes;
mod payload_object_free;
mod punned_ladder_policy;
mod punned_select;
mod punned_writeback;
mod paired_float_product_schedule;
mod pooled_float_literal_reuse;
mod repeated_integer_constant_reuse;
mod queue_callback_fold;
mod reciprocal_frame_fill_schedule;
mod resource_event_switch;
mod range_guarded_array_address;
mod register_inline_asm;
mod schedule_relocations;
mod basic_block_schedule;
mod scaled_angle_call;
mod variadic_report_member_schedule;
mod variadic_float_conversion_report_schedule;
mod variadic_report_loop_tail_schedule;
mod vec3_product_schedule;
mod wide_call_result_mask_chain;
mod sorted_intrusive_global_insert;
mod sorted_intrusive_insert;
mod store_fill;
mod store_return_schedule;
mod stack_trace_report_loop_schedule;
mod stored_guarded_global_callback;
mod shared_float_store_literal;
mod shared_mask_word;
mod switch_assignment_call_tail;
mod switch_call_dispatcher;
mod switch_call_return;
mod tail_call;
mod tokenizer;
mod toggled_guarded_global_callback;
mod trig_quadrant_dispatch;
mod unoptimized_integer_round_up;
mod variadic;
mod zero_call_forward;

pub(crate) use callee_saved::{
    plan_linkage_first_data_anchor, summarize_queue_pop, summarize_queue_service, QueuePopSummary,
    QueueServiceSummary,
};
pub(crate) use guarded_aggregate_update::{
    summarize_guarded_aggregate_update, GuardedAggregateUpdateSummary,
};
pub(crate) use guarded_float_table_index::{
    summarize_guarded_float_table_index, GuardedFloatTableIndexSummary,
};
use aggregate_return_temporaries::materialize_aggregate_return_temporaries;
pub(crate) use local_select::{
    summarize_unoptimized_local_select, UnoptimizedLocalSelectSummary,
};
pub(crate) use one_word_aggregate::source_proven_one_word_aggregate_locals;
pub(crate) use canonical_boolean::source_proven_canonical_boolean_locals;
#[allow(unused_imports)]
pub(crate) use passes::*;

pub(crate) use crate::analysis::*;
pub(crate) use member_float_normalize::lower_member_float_normalize;
pub(crate) use member_linefeed::lower_member_linefeed;
pub(crate) use member_rect_control::lower_member_rect_control;
pub(crate) use member_tab::lower_member_tab;
pub(crate) use register_inline_asm::lower_register_inline_asm_wrapper;
use trig_quadrant_dispatch::TrigQuadrant;
pub(crate) use crate::expressions::pointer_stride;
pub(crate) use crate::expressions::{
    const_address_of, const_address_pointer, displacement_store, pointee_of_type, split_address,
};
pub(crate) use crate::generator::*;
pub(crate) use long_long_support::{unsigned_word_clock, ClockRead};
pub(crate) use mwcc_core::{Compilation, Diagnostic};
pub(crate) use mwcc_machine_code::{Instruction, RelocationKind};
pub(crate) use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, LoopKind, Pointee,
    Statement, Type, UnaryOperator,
};
pub(crate) use mwcc_target::Eabi;
pub(crate) use mwcc_versions::{
    CallDispatcherStyle, FixedAddressConstantStoreStyle, FrameConvention, GlobalAddressing,
    GuardedMemberInitializationStyle,
    IntegerComparisonValueStyle, LongLongTimerStyle, NarrowComputedReturnStyle,
    NestedGlobalDispatchSchedule, PlainLinkageEpilogueStyle, PointerCallStoreEpilogueStyle,
    PointerWalkerScheduleStyle, RaiseFamilyStyle, WideConstantAddSchedule,
};
