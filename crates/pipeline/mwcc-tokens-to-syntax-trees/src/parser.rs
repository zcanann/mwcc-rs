//! The token cursor: the `Parser` state and its primitive operations.

use std::collections::HashMap;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::Type;
use mwcc_tokens::Token;

/// One resolved struct member: its type and byte offset within the struct, plus
/// the struct tag it points to when the member is itself a struct pointer (so
/// chained access `a->b->c` resolves).
pub(crate) struct StructField {
    pub(crate) member_type: Type,
    pub(crate) offset: u16,
    pub(crate) struct_tag: Option<String>,
}

/// A struct's layout: members by name, plus the total size (for `sizeof`/arrays,
/// later). Offsets follow natural alignment (the `-align powerpc` default).
#[derive(Default)]
pub(crate) struct StructLayout {
    pub(crate) fields: HashMap<String, StructField>,
}

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) position: usize,
    /// Declared struct layouts, by tag name.
    pub(crate) structs: HashMap<String, StructLayout>,
    /// In-scope variables that are struct pointers, mapped to their struct tag,
    /// so `variable->field` resolves to the right layout.
    pub(crate) variable_structs: HashMap<String, String>,
    /// Set by [`Parser::parse_type`] when it just parsed a `struct Name*`, so the
    /// declarator parser can associate the variable name with the struct tag.
    pub(crate) last_struct_tag: Option<String>,
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
