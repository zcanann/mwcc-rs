//! The expression tree.

use crate::operators::{BinaryOperator, UnaryOperator};
use crate::types::{Pointee, Type};

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
    /// `*pointer` — load the pointed-to value.
    Dereference {
        pointer: Box<Expression>,
    },
    /// `base[index]` — load `*(base + index)`, the index scaled by element size.
    Index {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    /// `base->field` (or `base.field`) — load the member at `offset` from the
    /// struct pointer `base`. The parser resolves the field to its byte offset and
    /// type from the struct layout, so codegen is a plain displacement access.
    Member {
        base: Box<Expression>,
        offset: u16,
        member_type: Type,
    },
    /// `base->arr` where `arr` is an array member — the *address* of the array
    /// (`base + offset`), an `element`-typed pointer that decays for subscripting.
    MemberAddress {
        base: Box<Expression>,
        offset: u16,
        element: Pointee,
    },
    /// `name(arguments)` — a direct call.
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
    /// `target = value` used as an expression — stores `value` into `target` and
    /// yields the stored value (e.g. `(g = g + 1)`).
    Assign {
        target: Box<Expression>,
        value: Box<Expression>,
    },
}
