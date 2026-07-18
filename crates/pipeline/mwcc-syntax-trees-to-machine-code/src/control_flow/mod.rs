//! Conditionals, float selects, short-circuit `&&`/`||`, branch tests.
//!
//! Split by family (fire 539); behavior-identical.

mod branches;
mod float;
mod passes;
mod record_condition;
mod select;

#[allow(unused_imports)]
pub(crate) use passes::*;

use crate::analysis::*;
use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
