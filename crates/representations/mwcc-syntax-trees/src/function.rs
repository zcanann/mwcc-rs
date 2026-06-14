//! Function definitions and the declarations that make up their bodies.

use crate::expression::Expression;
use crate::types::Type;

/// A function parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub parameter_type: Type,
    pub name: String,
}

/// A local variable declaration with an initializer: `type name = expression;`.
#[derive(Debug, Clone)]
pub struct LocalDeclaration {
    pub declared_type: Type,
    pub name: String,
    pub initializer: Expression,
}

/// A guarded early return: `if (condition) return value;`.
#[derive(Debug, Clone)]
pub struct GuardedReturn {
    pub condition: Expression,
    pub value: Expression,
}

/// A body statement (beyond declarations, guards, and the return).
#[derive(Debug, Clone)]
pub enum Statement {
    /// `*pointer = value;` or `base[index] = value;` — a store to memory. The
    /// target is a `Dereference` or `Index` expression.
    Store { target: Expression, value: Expression },
}

/// A function definition. Bodies are zero or more local declarations, then zero
/// or more statements, then zero or more `if (...) return ...;` guards, then an
/// optional final `return <expression>;` (absent for a `void` function).
#[derive(Debug, Clone)]
pub struct Function {
    pub return_type: Type,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub locals: Vec<LocalDeclaration>,
    pub statements: Vec<Statement>,
    pub guards: Vec<GuardedReturn>,
    pub return_expression: Option<Expression>,
}
