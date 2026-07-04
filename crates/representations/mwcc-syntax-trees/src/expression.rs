//! The expression tree.

use crate::operators::{BinaryOperator, UnaryOperator};
use crate::types::{Pointee, Type};

/// An expression.
#[derive(Debug, Clone)]
pub enum Expression {
    IntegerLiteral(i64),
    FloatLiteral(f64),
    /// A string literal in expression position (the bytes, without the trailing
    /// NUL) — pooled into an anonymous `@N` `.sdata` object; its use loads the
    /// object's address.
    StringLiteral(Vec<u8>),
    Variable(String),
    /// A braced aggregate initializer on a LOCAL (`decimal d = { 0, 0, { 0, "" } }`)
    /// — parsed for AST fidelity (the capture hash); general codegen defers on it.
    AggregateLiteral(Vec<Expression>),
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
    /// `&operand` — the address of an lvalue (a variable, dereference, member, or
    /// index). Taking a variable's address forces it to be frame-resident.
    AddressOf {
        operand: Box<Expression>,
    },
    /// `base[index]` — load `*(base + index)`, the index scaled by element size.
    Index {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    /// `base->field` (or `base.field`) — load the member at `offset` from the
    /// struct pointer `base`. The parser resolves the field to its byte offset and
    /// type from the struct layout, so codegen is a plain displacement access.
    /// `index_stride` is the struct size when `base` is an array/pointer index
    /// (`a[i].field`): codegen scales the index by it (`a + i*stride + offset`);
    /// `None` for a plain pointer base.
    Member {
        base: Box<Expression>,
        offset: u16,
        member_type: Type,
        index_stride: Option<u16>,
    },
    /// `base->arr` where `arr` is an array member — the *address* of the array
    /// (`base + offset`), an `element`-typed pointer that decays for subscripting.
    MemberAddress {
        base: Box<Expression>,
        offset: u16,
        element: Pointee,
    },
    /// `target(arguments)` where the callee is an EXPRESSION (a function-
    /// pointer struct member — `file->writeFunc(...)`): an indirect call
    /// through a computed address. General codegen defers (captures only).
    CallThrough {
        target: Box<Expression>,
        arguments: Vec<Expression>,
    },
    /// `name(arguments)` — a direct call.
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
    /// `target++` / `target--` — the POSTFIX step. Yields the OLD value,
    /// then increments. Kept distinct from the `Assign` desugar (which
    /// yields the NEW value) so value-position uses stay faithful;
    /// statement positions discard the value and lower to the Assign.
    PostStep {
        target: Box<Expression>,
        operator: BinaryOperator,
    },
    /// `target = value` used as an expression — stores `value` into `target` and
    /// yields the stored value (e.g. `(g = g + 1)`).
    Assign {
        target: Box<Expression>,
        value: Box<Expression>,
    },
    /// The comma operator `left, right`: evaluate `left` for its side effects, discard
    /// its value, then yield `right`. A side-effect-free `left` emits nothing.
    Comma {
        left: Box<Expression>,
        right: Box<Expression>,
    },
}
