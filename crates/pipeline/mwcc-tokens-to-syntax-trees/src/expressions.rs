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

        match self.advance() {
            Token::IntegerLiteral(value) => Ok(Expression::IntegerLiteral(value)),
            Token::FloatLiteral(value) => Ok(Expression::FloatLiteral(value)),
            Token::Identifier(name) => Ok(Expression::Variable(name)),
            Token::ParenOpen => {
                // `(type) expr` is a cast; otherwise a parenthesised expression.
                if self.peek_is_type() {
                    let target_type = self.parse_type()?;
                    self.expect(Token::ParenClose)?;
                    let operand = self.factor()?;
                    Ok(Expression::Cast { target_type, operand: Box::new(operand) })
                } else {
                    let inner = self.expression()?;
                    self.expect(Token::ParenClose)?;
                    Ok(inner)
                }
            }
            other => Err(Diagnostic::error(format!("expected an expression, found {other}"))),
        }
    }
}
