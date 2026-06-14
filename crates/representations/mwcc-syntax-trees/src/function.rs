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

/// A function definition. Bodies are zero or more local declarations, then zero
/// or more `if (...) return ...;` guards, then a final `return <expression>;`.
#[derive(Debug, Clone)]
pub struct Function {
    pub return_type: Type,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub locals: Vec<LocalDeclaration>,
    pub guards: Vec<GuardedReturn>,
    pub return_expression: Expression,
}
