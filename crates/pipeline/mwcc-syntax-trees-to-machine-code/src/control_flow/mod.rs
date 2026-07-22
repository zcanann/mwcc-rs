//! Conditionals, float selects, short-circuit `&&`/`||`, branch tests.
//!
//! Split by family (fire 539); behavior-identical.

mod absolute_value;
mod branch_preserving_select;
mod branches;
mod common_offset_select;
mod float;
mod nested_phi_select;
mod negated_short_circuit;
mod passes;
mod record_condition;
mod record_mask;
mod select;

#[allow(unused_imports)]
pub(crate) use passes::*;

use crate::analysis::*;
use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, ConditionalOrigin, Expression, UnaryOperator};
