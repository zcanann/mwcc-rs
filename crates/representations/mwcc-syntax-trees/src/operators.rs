//! Binary and prefix-unary operators, with their binding strength.

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
    LogicalAnd,
    LogicalOr,
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
            BinaryOperator::LogicalAnd => 2,
            BinaryOperator::LogicalOr => 1,
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
