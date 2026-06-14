//! The syntax-tree representation: the parsed shape of a translation unit,
//! before any semantic analysis or lowering.

/// A source-level type in the supported subset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Int,
    Float,
    Void,
}

/// A function parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub parameter_type: Type,
    pub name: String,
}

/// An arithmetic operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expression {
    IntegerLiteral(i64),
    FloatLiteral(f64),
    Variable(String),
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
}

/// A local variable declaration with an initializer: `type name = expression;`.
#[derive(Debug, Clone)]
pub struct LocalDeclaration {
    pub declared_type: Type,
    pub name: String,
    pub initializer: Expression,
}

/// A function definition. Bodies are zero or more local declarations followed by
/// a single `return <expression>;`.
#[derive(Debug, Clone)]
pub struct Function {
    pub return_type: Type,
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub locals: Vec<LocalDeclaration>,
    pub return_expression: Expression,
}
