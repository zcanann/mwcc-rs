//! The syntax-tree representation: the parsed shape of a translation unit,
//! before any semantic analysis or lowering.

/// A source-level type in the supported subset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Int,
    UnsignedInt,
    Char,
    UnsignedChar,
    Short,
    UnsignedShort,
    Float,
    Void,
}

impl Type {
    /// Whether this is a signed integer (affects e.g. `>>` and narrowing).
    pub fn is_signed(self) -> bool {
        matches!(self, Type::Int | Type::Char | Type::Short)
    }

    /// Integer width in bits (8/16/32); 32 for non-narrow types.
    pub fn width(self) -> u8 {
        match self {
            Type::Char | Type::UnsignedChar => 8,
            Type::Short | Type::UnsignedShort => 16,
            _ => 32,
        }
    }
}

/// A function parameter.
#[derive(Debug, Clone)]
pub struct Parameter {
    pub parameter_type: Type,
    pub name: String,
}

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Equal,
    NotEqual,
}

impl BinaryOperator {
    /// Binding strength (higher binds tighter), matching C for these operators.
    pub fn precedence(self) -> u8 {
        match self {
            BinaryOperator::Multiply | BinaryOperator::Divide | BinaryOperator::Modulo => 10,
            BinaryOperator::Add | BinaryOperator::Subtract => 9,
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight => 8,
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => 7,
            BinaryOperator::Equal | BinaryOperator::NotEqual => 6,
            BinaryOperator::BitAnd => 5,
            BinaryOperator::BitXor => 4,
            BinaryOperator::BitOr => 3,
        }
    }
}

/// A prefix unary operator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOperator {
    Negate,
    BitNot,
    LogicalNot,
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
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Conditional {
        condition: Box<Expression>,
        when_true: Box<Expression>,
        when_false: Box<Expression>,
    },
    Cast {
        target_type: Type,
        operand: Box<Expression>,
    },
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
