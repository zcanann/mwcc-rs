//! Precedence-climbing expression parsing: ternary, binary operators, prefix
//! unary operators, casts, and primary factors.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use mwcc_tokens::Token;

use crate::parser::Parser;

impl Parser {
    pub(crate) fn expression(&mut self) -> Compilation<Expression> {
        let condition = self.binary_expression(1)?;
        // ternary conditional has the lowest precedence
        if *self.peek() == Token::Question {
            self.advance();
            let when_true = self.expression()?;
            self.expect(Token::Colon)?;
            let when_false = self.expression()?;
            return Ok(Expression::Conditional {
                condition: Box::new(condition),
                when_true: Box::new(when_true),
                when_false: Box::new(when_false),
            });
        }
        Ok(condition)
    }

    /// Precedence-climbing parse of left-associative binary operators with
    /// precedence at least `minimum`.
    pub(crate) fn binary_expression(&mut self, minimum: u8) -> Compilation<Expression> {
        let mut left = self.factor()?;
        while let Some(operator) = self.peek_binary_operator() {
            if operator.precedence() < minimum {
                break;
            }
            self.advance();
            let right = self.binary_expression(operator.precedence() + 1)?;
            left = Expression::Binary { operator, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    pub(crate) fn peek_binary_operator(&self) -> Option<BinaryOperator> {
        Some(match self.peek() {
            Token::Plus => BinaryOperator::Add,
            Token::Minus => BinaryOperator::Subtract,
            Token::Star => BinaryOperator::Multiply,
            Token::Slash => BinaryOperator::Divide,
            Token::Percent => BinaryOperator::Modulo,
            Token::Ampersand => BinaryOperator::BitAnd,
            Token::Pipe => BinaryOperator::BitOr,
            Token::Caret => BinaryOperator::BitXor,
            Token::ShiftLeft => BinaryOperator::ShiftLeft,
            Token::ShiftRight => BinaryOperator::ShiftRight,
            Token::Less => BinaryOperator::Less,
            Token::Greater => BinaryOperator::Greater,
            Token::LessEqual => BinaryOperator::LessEqual,
            Token::GreaterEqual => BinaryOperator::GreaterEqual,
            Token::EqualEqual => BinaryOperator::Equal,
            Token::BangEqual => BinaryOperator::NotEqual,
            Token::AmpersandAmpersand => BinaryOperator::LogicalAnd,
            Token::PipePipe => BinaryOperator::LogicalOr,
            _ => return None,
        })
    }

    pub(crate) fn factor(&mut self) -> Compilation<Expression> {
        // prefix dereference: `*pointer`
        if *self.peek() == Token::Star {
            self.advance();
            let pointer = self.factor()?;
            return Ok(Expression::Dereference { pointer: Box::new(pointer) });
        }
        // prefix unary operators
        let unary = match self.peek() {
            Token::Minus => Some(UnaryOperator::Negate),
            Token::Tilde => Some(UnaryOperator::BitNot),
            Token::Bang => Some(UnaryOperator::LogicalNot),
            _ => None,
        };
        if let Some(operator) = unary {
            self.advance();
            let operand = self.factor()?;
            return Ok(Expression::Unary { operator, operand: Box::new(operand) });
        }

        let mut expression = match self.advance() {
            Token::IntegerLiteral(value) => Expression::IntegerLiteral(value),
            Token::FloatLiteral(value) => Expression::FloatLiteral(value),
            // `name(args)` is a call; a bare `name` is a variable.
            Token::Identifier(name) if *self.peek() == Token::ParenOpen => {
                self.advance();
                let mut arguments = Vec::new();
                if *self.peek() != Token::ParenClose {
                    loop {
                        arguments.push(self.expression()?);
                        if *self.peek() == Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(Token::ParenClose)?;
                Expression::Call { name, arguments }
            }
            Token::Identifier(name) => Expression::Variable(name),
            Token::ParenOpen => {
                // `(type) expr` is a cast; otherwise a parenthesised expression.
                if self.peek_is_type() {
                    let target_type = self.parse_type()?;
                    self.expect(Token::ParenClose)?;
                    let operand = self.factor()?;
                    Expression::Cast { target_type, operand: Box::new(operand) }
                } else {
                    let inner = self.expression()?;
                    self.expect(Token::ParenClose)?;
                    inner
                }
            }
            other => return Err(Diagnostic::error(format!("expected an expression, found {other}"))),
        };
        // postfix subscript `base[index]` and member access `base->field` /
        // `base.field`, left-associative.
        loop {
            match self.peek() {
                Token::BracketOpen => {
                    self.advance();
                    let index = self.expression()?;
                    self.expect(Token::BracketClose)?;
                    expression = Expression::Index { base: Box::new(expression), index: Box::new(index) };
                }
                Token::Arrow | Token::Dot => {
                    self.advance();
                    let field = self.parse_identifier()?;
                    expression = self.resolve_member(expression, &field)?;
                }
                _ => break,
            }
        }
        Ok(expression)
    }

    /// Resolve `base.field` / `base->field` to a [`Expression::Member`] with the
    /// member's baked offset and type. The base must be a variable known to be a
    /// pointer to a declared struct (single-level access; chains arrive later).
    fn resolve_member(&self, base: Expression, field: &str) -> Compilation<Expression> {
        let tag = match &base {
            Expression::Variable(name) => self.variable_structs.get(name).cloned(),
            _ => None,
        };
        let tag = tag.ok_or_else(|| Diagnostic::error(format!("member '{field}' on a non-struct-pointer base")))?;
        let layout = self.structs.get(&tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
        let member = layout
            .fields
            .get(field)
            .ok_or_else(|| Diagnostic::error(format!("struct '{tag}' has no member '{field}'")))?;
        Ok(Expression::Member { base: Box::new(base), offset: member.offset, member_type: member.member_type })
    }
}
