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
    /// `local = value;` — reassignment of a local variable (value-tracked, not a
    /// memory store).
    Assign { name: String, value: Expression },
    /// A bare expression evaluated for its side effects, e.g. `g();`.
    Expression(Expression),
}

/// A file-scope global variable, e.g. `int g;`, `extern int g;`, or `static int
/// g;`. A non-`extern` declaration is a (tentative) definition that the object
/// places in a data section; an `extern` one is just a reference to a symbol
/// defined elsewhere. `array_length` is set for `type g[N];`.
#[derive(Debug, Clone)]
pub struct GlobalDeclaration {
    pub declared_type: Type,
    pub name: String,
    pub is_extern: bool,
    pub is_static: bool,
    pub array_length: Option<u16>,
    /// A scalar `= <constant>` initializer's value. `Some(non-zero)` places the
    /// global in `.sdata` (initialized data); `None` or `Some(0)` leaves it in
    /// `.sbss` (zero-initialized).
    pub initializer: Option<i64>,
}

/// A translation unit: file-scope globals (and skipped prototypes) interleaved
/// with one or more function definitions, in source order.
#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub globals: Vec<GlobalDeclaration>,
    pub functions: Vec<Function>,
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
