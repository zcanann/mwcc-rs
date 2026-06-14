//! The token cursor: the `Parser` state and its primitive operations.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_tokens::Token;

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) position: usize,
}

impl Parser {
    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }
    pub(crate) fn advance(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }
    pub(crate) fn expect(&mut self, expected: Token) -> Compilation<()> {
        if *self.peek() == expected {
            self.position += 1;
            Ok(())
        } else {
            Err(Diagnostic::error(format!("expected {expected}, found {}", self.peek())))
        }
    }

    pub(crate) fn parse_identifier(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(name) => Ok(name),
            other => Err(Diagnostic::error(format!("expected an identifier, found {other}"))),
        }
    }
}
