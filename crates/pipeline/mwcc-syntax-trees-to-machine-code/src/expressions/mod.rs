//! Core integer expression evaluation and operand placement.
//!
//! Split by family (fire 528); behavior-identical to the former single expressions.rs.

mod arithmetic;
mod bit_fields;
mod calls;
mod driver;
mod global_array_decay;
mod global_array_index;
mod globals;
mod implicit_narrow_store;
mod indexed_rmw;
mod members;
mod narrow_compound;
mod operands;
mod passes;
mod pointers;
mod stores;
mod strings;

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
    BitFieldLoadPlacement, GlobalAddressing, GlobalArrayDecayStoreStyle,
    MaterializationCopyStyle,
};
