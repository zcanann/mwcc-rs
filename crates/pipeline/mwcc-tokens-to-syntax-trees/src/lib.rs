//! Pipeline: tokens -> syntax trees (parsing).
//!
//! Recursive-descent over the v0 grammar:
//!   function   := type identifier '(' parameters ')' '{' 'return' expression ';' '}'
//!   expression := term (('+' | '-') term)*
//!   term       := factor (('*' | '/') factor)*
//!   factor     := literal | identifier | '(' expression ')'

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LocalDeclaration, Parameter, Type};
use mwcc_tokens::Token;

pub fn parse_function(tokens: Vec<Token>) -> Compilation<Function> {
    let mut parser = Parser { tokens, position: 0 };
    parser.function()
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }
    fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }
    fn expect(&mut self, expected: Token) -> Compilation<()> {
        if *self.peek() == expected {
            self.position += 1;
            Ok(())
        } else {
            Err(Diagnostic::error(format!("expected {expected}, found {}", self.peek())))
        }
    }

    fn parse_type(&mut self) -> Compilation<Type> {
        match self.advance() {
            Token::KeywordInt => Ok(Type::Int),
            Token::KeywordFloat => Ok(Type::Float),
            Token::KeywordVoid => Ok(Type::Void),
            other => Err(Diagnostic::error(format!("expected a type, found {other}"))),
        }
    }

    fn parse_identifier(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(name) => Ok(name),
            other => Err(Diagnostic::error(format!("expected an identifier, found {other}"))),
        }
    }

    fn function(&mut self) -> Compilation<Function> {
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

        self.expect(Token::KeywordReturn)?;
        let return_expression = self.expression()?;
        self.expect(Token::Semicolon)?;
        self.expect(Token::BraceClose)?;

        Ok(Function { return_type, name, parameters, locals, return_expression })
    }

    fn peek_is_type(&self) -> bool {
        matches!(self.peek(), Token::KeywordInt | Token::KeywordFloat | Token::KeywordVoid)
    }

    fn expression(&mut self) -> Compilation<Expression> {
        let mut left = self.term()?;
        loop {
            let operator = match self.peek() {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Subtract,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            left = Expression::Binary { operator, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn term(&mut self) -> Compilation<Expression> {
        let mut left = self.factor()?;
        loop {
            let operator = match self.peek() {
                Token::Star => BinaryOperator::Multiply,
                Token::Slash => BinaryOperator::Divide,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            left = Expression::Binary { operator, left: Box::new(left), right: Box::new(right) };
        }
        Ok(left)
    }

    fn factor(&mut self) -> Compilation<Expression> {
        match self.advance() {
            Token::IntegerLiteral(value) => Ok(Expression::IntegerLiteral(value)),
            Token::FloatLiteral(value) => Ok(Expression::FloatLiteral(value)),
            Token::Identifier(name) => Ok(Expression::Variable(name)),
            Token::ParenOpen => {
                let inner = self.expression()?;
                self.expect(Token::ParenClose)?;
                Ok(inner)
            }
            other => Err(Diagnostic::error(format!("expected an expression, found {other}"))),
        }
    }
}
