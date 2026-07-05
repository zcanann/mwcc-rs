//! Conditionals, float selects, short-circuit `&&`/`||`, branch tests.
//!
//! Split by family (fire 539); behavior-identical.

mod passes;
mod select;
mod branches;
mod float;

#[allow(unused_imports)]
pub(crate) use passes::*;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;
