//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Function, GuardedReturn, LocalDeclaration, Parameter, Type};
use mwcc_tokens::Token;

use crate::parser::Parser;

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        match self.advance() {
            Token::KeywordInt => Ok(Type::Int),
            Token::KeywordChar => Ok(Type::Char),
            Token::KeywordShort => Ok(Type::Short),
            // `unsigned` may be followed by char/short/int.
            Token::KeywordUnsigned => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Ok(Type::UnsignedChar)
                }
                Token::KeywordShort => {
                    self.advance();
                    Ok(Type::UnsignedShort)
                }
                Token::KeywordInt => {
                    self.advance();
                    Ok(Type::UnsignedInt)
                }
                _ => Ok(Type::UnsignedInt),
            },
            Token::KeywordFloat => Ok(Type::Float),
            Token::KeywordVoid => Ok(Type::Void),
            other => Err(Diagnostic::error(format!("expected a type, found {other}"))),
        }
    }

    pub(crate) fn function(&mut self) -> Compilation<Function> {
        let return_type = self.parse_type()?;
        let name = self.parse_identifier()?;
        self.expect(Token::ParenOpen)?;

        let mut parameters = Vec::new();
        if *self.peek() == Token::KeywordVoid {
            self.advance();
        } else if *self.peek() != Token::ParenClose {
            loop {
                let parameter_type = self.parse_type()?;
                let name = self.parse_identifier()?;
                parameters.push(Parameter { parameter_type, name });
                if *self.peek() == Token::Comma {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        self.expect(Token::ParenClose)?;
        self.expect(Token::BraceOpen)?;

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        while self.peek_is_type() {
            let declared_type = self.parse_type()?;
            let name = self.parse_identifier()?;
            self.expect(Token::Equals)?;
            let initializer = self.expression()?;
            self.expect(Token::Semicolon)?;
            locals.push(LocalDeclaration { declared_type, name, initializer });
        }

        // Zero or more guarded early returns: `if (condition) return value;`.
        let mut guards = Vec::new();
        while *self.peek() == Token::KeywordIf {
            self.advance();
            self.expect(Token::ParenOpen)?;
            let condition = self.expression()?;
            self.expect(Token::ParenClose)?;
            self.expect(Token::KeywordReturn)?;
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            guards.push(GuardedReturn { condition, value });
        }

        self.expect(Token::KeywordReturn)?;
        let return_expression = self.expression()?;
        self.expect(Token::Semicolon)?;
        self.expect(Token::BraceClose)?;

        Ok(Function { return_type, name, parameters, locals, guards, return_expression })
    }

    pub(crate) fn peek_is_type(&self) -> bool {
        matches!(
            self.peek(),
            Token::KeywordInt
                | Token::KeywordChar
                | Token::KeywordShort
                | Token::KeywordUnsigned
                | Token::KeywordFloat
                | Token::KeywordVoid
        )
    }
}
