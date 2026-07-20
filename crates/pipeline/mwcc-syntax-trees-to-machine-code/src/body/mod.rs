//! Function-level emission: parameters, body, guards, and the return tail.
//!
//! Split by family (fire 525); behavior-identical to the former single body.rs.

mod call_prologue;
mod callee_saved;
mod comma_operator;
mod condition_linkage;
mod conditional;
mod dispatchers;
mod driver;
mod enum_remap_member_update;
mod float_store_fill;
mod fixed_address_object_flush;
mod fixed_port_bitfield;
mod fixed_port_indexed_bitfield;
mod fixed_port_replay_update;
mod guards_ifs;
mod if_else;
mod indirect_call;
mod ladders;
mod legacy_constant_store;
mod loops;
mod passes;
mod punned_ladder_policy;
mod punned_select;
mod punned_writeback;
mod queue_callback_fold;
mod sorted_intrusive_insert;
mod store_fill;
mod store_return_schedule;
mod switch_assignment_call_tail;
mod switch_call_dispatcher;
mod tail_call;

pub(crate) use callee_saved::{
    summarize_queue_pop, summarize_queue_service, QueuePopSummary, QueueServiceSummary,
};
#[allow(unused_imports)]
pub(crate) use passes::*;

pub(crate) use crate::analysis::*;
pub(crate) use crate::expressions::pointer_stride;
pub(crate) use crate::expressions::{displacement_store, pointee_of_type};
pub(crate) use crate::generator::*;
pub(crate) use mwcc_core::{Compilation, Diagnostic};
pub(crate) use mwcc_machine_code::{Instruction, RelocationKind};
pub(crate) use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, LoopKind, Pointee,
    Statement, Type, UnaryOperator,
};
pub(crate) use mwcc_target::Eabi;
pub(crate) use mwcc_versions::{
    FrameConvention, GlobalAddressing, IntegerComparisonValueStyle, NarrowComputedReturnStyle,
    PointerWalkerScheduleStyle, RaiseFamilyStyle, WideConstantAddSchedule,
};
