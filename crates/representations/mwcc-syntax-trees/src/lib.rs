//! The syntax-tree representation: the parsed shape of a translation unit,
//! before any semantic analysis or lowering. `lib.rs` only re-exports the
//! representation modules.

mod expression;
mod function;
mod operators;
mod types;

pub use expression::Expression;
pub use function::{ArmBody, AsmInstruction, AsmItem, AsmOperand, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter, PointerElement, Statement, SwitchArm, TranslationUnit};
pub use operators::{BinaryOperator, UnaryOperator};
pub use types::{Pointee, Type};
