//! The syntax-tree representation: the parsed shape of a translation unit,
//! before any semantic analysis or lowering. `lib.rs` only re-exports the
//! representation modules.

mod aggregate;
mod expression;
mod function;
mod operators;
mod types;

pub use aggregate::{AggregateDefinition, AggregateMember, SourceFundamentalType};
pub use expression::{ConditionalOrigin, Expression};
pub use function::{
    ArmBody, AsmInstruction, AsmItem, AsmOperand, AsmRelocSuffix, CxxAbiBase, CxxAbiClass,
    CxxAbiVtableComponent, CxxInlineOrdinalFacts, Function, FunctionSource, GlobalDeclaration,
    GuardedReturn, InlineExpansionFacts, LocalDeclaration, LoopKind, Parameter, PointerElement,
    Statement, SwitchArm, TranslationUnit,
};
pub use operators::{BinaryOperator, UnaryOperator};
pub use types::{Pointee, Type};
