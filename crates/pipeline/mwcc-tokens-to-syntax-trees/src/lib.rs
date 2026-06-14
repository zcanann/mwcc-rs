//! Pipeline: tokens -> syntax trees (parsing).
//!
//! Recursive-descent over the v0 grammar:
//!   function   := type identifier '(' parameters ')' '{' 'return' expression ';' '}'
//!   expression := term (('+' | '-') term)*
//!   term       := factor (('*' | '/') factor)*
//!   factor     := literal | identifier | '(' expression ')'

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LocalDeclaration, Parameter, Type, UnaryOperator};
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
            // `unsigned` and `unsigned int` both mean unsigned int.
            Token::KeywordUnsigned => {
                if *self.peek() == Token::KeywordInt {
                    self.advance();
                }
                Ok(Type::UnsignedInt)
            }
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
        matches!(self.peek(), Token::KeywordInt | Token::KeywordUnsigned | Token::KeywordFloat | Token::KeywordVoid)
    }

    fn expression(&mut self) -> Compilation<Expression> {
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
    fn binary_expression(&mut self, minimum: u8) -> Compilation<Expression> {
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

    fn peek_binary_operator(&self) -> Option<BinaryOperator> {
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
            _ => return None,
        })
    }

    fn factor(&mut self) -> Compilation<Expression> {
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
                let inner = self.expression()?;
                self.expect(Token::ParenClose)?;
                Ok(inner)
            }
            other => Err(Diagnostic::error(format!("expected an expression, found {other}"))),
        }
    }
}
