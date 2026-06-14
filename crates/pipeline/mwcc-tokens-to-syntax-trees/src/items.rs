//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Function, GuardedReturn, LocalDeclaration, Parameter, Pointee, Statement, Type};
use mwcc_tokens::Token;

use crate::parser::Parser;

/// The pointee kind for `<scalar>*`. Pointer-to-pointer and pointer-to-aggregate
/// are not in the subset yet.
fn pointee_of(base: Type) -> Compilation<Pointee> {
    match base {
        Type::Int => Ok(Pointee::Int),
        Type::UnsignedInt => Ok(Pointee::UnsignedInt),
        Type::Char => Ok(Pointee::Char),
        Type::UnsignedChar => Ok(Pointee::UnsignedChar),
        Type::Short => Ok(Pointee::Short),
        Type::UnsignedShort => Ok(Pointee::UnsignedShort),
        Type::Float => Ok(Pointee::Float),
        other => Err(Diagnostic::error(format!("pointer to {other:?} is not supported yet"))),
    }
}

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Compilation<Type> {
        let base = match self.advance() {
            Token::KeywordInt => Type::Int,
            Token::KeywordChar => Type::Char,
            Token::KeywordShort => Type::Short,
            // `unsigned` may be followed by char/short/int.
            Token::KeywordUnsigned => match self.peek() {
                Token::KeywordChar => {
                    self.advance();
                    Type::UnsignedChar
                }
                Token::KeywordShort => {
                    self.advance();
                    Type::UnsignedShort
                }
                Token::KeywordInt => {
                    self.advance();
                    Type::UnsignedInt
                }
                _ => Type::UnsignedInt,
            },
            Token::KeywordFloat => Type::Float,
            Token::KeywordVoid => Type::Void,
            other => return Err(Diagnostic::error(format!("expected a type, found {other}"))),
        };
        // A trailing `*` makes it a pointer to that scalar.
        if *self.peek() == Token::Star {
            self.advance();
            return Ok(Type::Pointer(pointee_of(base)?));
        }
        Ok(base)
    }

    pub(crate) fn function(&mut self) -> Compilation<Function> {
        // Skip leading prototype declarations (`type name(params);`) until the
        // function *definition* (the signature followed by `{`).
        let (return_type, name, parameters) = loop {
            let return_type = self.parse_type()?;
            let name = self.parse_identifier()?;
            self.expect(Token::ParenOpen)?;

            let mut parameters = Vec::new();
            if *self.peek() == Token::KeywordVoid {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    let parameter_type = self.parse_type()?;
                    // The name is optional (a prototype may write just the type).
                    let name = if matches!(self.peek(), Token::Identifier(_)) {
                        self.parse_identifier()?
                    } else {
                        String::new()
                    };
                    parameters.push(Parameter { parameter_type, name });
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;

            if *self.peek() == Token::Semicolon {
                self.advance(); // a prototype — keep looking for the definition
                continue;
            }
            break (return_type, name, parameters);
        };

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

        // Zero or more statements: a store `*p = v;` / `p[i] = v;`, or a bare
        // expression evaluated for effect like a call `g();`.
        let mut statements = Vec::new();
        while !matches!(self.peek(), Token::KeywordReturn | Token::KeywordIf | Token::BraceClose) {
            let first = self.factor()?;
            if *self.peek() == Token::Equals {
                self.advance();
                let value = self.expression()?;
                self.expect(Token::Semicolon)?;
                statements.push(Statement::Store { target: first, value });
            } else {
                self.expect(Token::Semicolon)?;
                statements.push(Statement::Expression(first));
            }
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

        // The final `return <expr>;` is optional — a `void` function may end after
        // its statements.
        let return_expression = if *self.peek() == Token::KeywordReturn {
            self.advance();
            let value = self.expression()?;
            self.expect(Token::Semicolon)?;
            Some(value)
        } else {
            None
        };
        self.expect(Token::BraceClose)?;

        Ok(Function { return_type, name, parameters, locals, statements, guards, return_expression })
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
