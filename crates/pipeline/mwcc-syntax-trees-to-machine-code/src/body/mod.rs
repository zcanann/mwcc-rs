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
mod float_store_fill;
mod guards_ifs;
mod if_else;
mod indirect_call;
mod ladders;
mod legacy_constant_store;
mod loops;
mod passes;
mod punned_select;
mod punned_writeback;
mod store_fill;
mod store_return_schedule;

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
    RaiseFamilyStyle, WideConstantAddSchedule,
};
