//! Function-level emission: parameters, body, guards, and the return tail.
//!
//! Split by family (fire 525); behavior-identical to the former single body.rs.

mod passes;
mod driver;
mod guards_ifs;
mod if_else;
mod conditional;
mod store_fill;
mod punned_select;
mod punned_writeback;
mod loops;
mod ladders;
mod dispatchers;
mod callee_saved;

#[allow(unused_imports)]
pub(crate) use passes::*;

pub(crate) use mwcc_core::{Compilation, Diagnostic};
pub(crate) use mwcc_machine_code::{Instruction, RelocationKind};
pub(crate) use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, LoopKind, Pointee, Statement, Type, UnaryOperator};
pub(crate) use mwcc_versions::GlobalAddressing;
pub(crate) use crate::expressions::{displacement_store, pointee_of_type};
pub(crate) use mwcc_target::Eabi;
pub(crate) use crate::analysis::*;
pub(crate) use crate::expressions::pointer_stride;
pub(crate) use crate::generator::*;
