//! MWCC assembler integer arithmetic is signed-word arithmetic, independent
//! of the C literal suffix. Out-of-width shifts saturate and division/modulo
//! by zero retain the dividend; these are measured assembler behaviors.
use crate::parser::Parser;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::BinaryOperator;
use mwcc_tokens::Token;

pub(super) fn starts_constant(token: &Token) -> bool {
    matches!(
        token,
        Token::IntegerLiteral(_)
            | Token::UnsignedIntegerLiteral(_)
            | Token::LongLongIntegerLiteral(_)
            | Token::UnsignedLongLongIntegerLiteral(_)
            | Token::Minus
            | Token::Plus
            | Token::Tilde
            | Token::Bang
            | Token::ParenOpen
    )
}

impl Parser {
    pub(super) fn parse_asm_constant(&mut self, minimum: u8) -> Compilation<i32> {
        let mut value = self.parse_asm_constant_atom()?;
        while let Some(operator) = self.peek_binary_operator() {
            if operator.precedence() < minimum {
                break;
            }
            self.advance();
            let right = self.parse_asm_constant(operator.precedence() + 1)?;
            value = binary(operator, value, right);
        }
        Ok(value)
    }

    fn parse_asm_constant_atom(&mut self) -> Compilation<i32> {
        Ok(match self.advance() {
            Token::IntegerLiteral(value)
            | Token::UnsignedIntegerLiteral(value)
            | Token::LongLongIntegerLiteral(value)
            | Token::UnsignedLongLongIntegerLiteral(value) => value as i32,
            Token::Minus => self.parse_asm_constant_atom()?.wrapping_neg(),
            Token::Plus => self.parse_asm_constant_atom()?,
            Token::Tilde => !self.parse_asm_constant_atom()?,
            Token::Bang => i32::from(self.parse_asm_constant_atom()? == 0),
            Token::ParenOpen => {
                let value = self.parse_asm_constant(0)?;
                self.expect(Token::ParenClose)?;
                value
            }
            Token::Identifier(name) if self.enum_constants.contains_key(&name) => {
                self.enum_constants[&name] as i32
            }
            other => {
                return Err(Diagnostic::error(format!(
                    "expected an asm integer constant, found {other}"
                )))
            }
        })
    }
}

fn binary(operator: BinaryOperator, left: i32, right: i32) -> i32 {
    use BinaryOperator::*;
    match operator {
        Add => left.wrapping_add(right),
        Subtract => left.wrapping_sub(right),
        Multiply => left.wrapping_mul(right),
        Divide | Modulo if right == 0 => left,
        Divide => left.wrapping_div(right),
        Modulo => left.wrapping_rem(right),
        BitAnd => left & right,
        BitOr => left | right,
        BitXor => left ^ right,
        ShiftLeft => {
            if (right as u32) < 32 {
                left.wrapping_shl(right as u32)
            } else {
                0
            }
        }
        ShiftRight => left >> (right as u32).min(31),
        Equal => i32::from(left == right),
        NotEqual => i32::from(left != right),
        Less => i32::from(left < right),
        LessEqual => i32::from(left <= right),
        Greater => i32::from(left > right),
        GreaterEqual => i32::from(left >= right),
        LogicalAnd => i32::from(left != 0 && right != 0),
        LogicalOr => i32::from(left != 0 || right != 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reproduces_word_shift_and_zero_divisor_rules() {
        assert_eq!(binary(BinaryOperator::ShiftRight, -1, 63), -1);
        assert_eq!(binary(BinaryOperator::ShiftLeft, 1, 32), 0);
        assert_eq!(binary(BinaryOperator::ShiftLeft, 1, -1), 0);
        assert_eq!(binary(BinaryOperator::Divide, 5, 0), 5);
        assert_eq!(binary(BinaryOperator::Modulo, -5, 0), -5);
        assert_eq!(binary(BinaryOperator::Divide, i32::MIN, -1), i32::MIN);
    }
}
