//! Function-level emission: parameters, body, guards, and the return tail.
//!
//! Split by family (fire 525); behavior-identical to the former single body.rs.

mod call_prologue;
mod leading_member_store_call;
mod leading_float_update_clamp;
mod aggregate_return_temporaries;
mod ascii_pointer_compare;
mod assertion_expression;
mod bounded_member_cursor;
mod callee_saved;
mod comma_operator;
mod condition_linkage;
mod conditional;
mod conditional_member_copy;
mod conditional_member_select_tail;
mod control_block_unique_copy;
mod dispatchers;
mod driver;
mod enum_remap_member_update;
mod endian_stack_pack;
mod endian_stack_unpack;
mod expression_statement;
mod float_store_fill;
mod float_friction_select;
mod fixed_address_object_flush;
mod fixed_port_bitfield;
mod fixed_port_indexed_bitfield;
mod fixed_port_replay_update;
mod guards_ifs;
mod global_struct_member_search;
mod guarded_aggregate_update;
mod guarded_member_decrement_if_else;
mod if_else;
mod indirect_call;
mod inlined_guarded_aggregate_update;
mod inlined_local_select;
mod ladders;
mod legacy_constant_store;
mod leading_store_guard;
mod leading_store_guarded_call;
mod local_select;
mod local_member_call_dispatch;
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
mod member_tab;
mod member_store_fill;
mod materialized_store_locals;
mod nested_global_indirect_call;
mod passes;
mod punned_ladder_policy;
mod punned_select;
mod punned_writeback;
mod queue_callback_fold;
mod range_guarded_array_address;
mod register_inline_asm;
mod sorted_intrusive_insert;
mod store_fill;
mod store_return_schedule;
mod switch_assignment_call_tail;
mod switch_call_dispatcher;
mod switch_call_return;
mod tail_call;
mod tokenizer;
mod trig_quadrant_dispatch;
mod unoptimized_integer_round_up;
mod variadic;

pub(crate) use callee_saved::{
    summarize_queue_pop, summarize_queue_service, QueuePopSummary, QueueServiceSummary,
};
pub(crate) use guarded_aggregate_update::{
    summarize_guarded_aggregate_update, GuardedAggregateUpdateSummary,
};
use aggregate_return_temporaries::materialize_aggregate_return_temporaries;
pub(crate) use local_select::{
    summarize_unoptimized_local_select, UnoptimizedLocalSelectSummary,
};
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
    const_address_pointer, displacement_store, pointee_of_type, split_address,
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
