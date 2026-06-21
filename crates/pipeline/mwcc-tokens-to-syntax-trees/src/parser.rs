//! The token cursor: the `Parser` state and its primitive operations.

use std::collections::HashMap;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;

/// One resolved struct member: its type and byte offset within the struct, plus
/// the struct tag it points to when the member is itself a struct pointer (so
/// chained access `a->b->c` resolves), or the element type when it is an array.
pub(crate) struct StructField {
    pub(crate) member_type: Type,
    pub(crate) offset: u16,
    pub(crate) struct_tag: Option<String>,
    pub(crate) array_element: Option<Pointee>,
}

/// A struct's layout: members by name, plus the total size (for `sizeof`/arrays,
/// later). Offsets follow natural alignment (the `-align powerpc` default).
#[derive(Default)]
pub(crate) struct StructLayout {
    pub(crate) fields: HashMap<String, StructField>,
    /// The struct's total size in bytes (members plus trailing padding to the
    /// struct's alignment) — the stride for an array/pointer of this struct.
    pub(crate) size: u16,
    /// The struct's alignment (the max member alignment) — a struct value's stack
    /// slot is aligned to this, not to its size.
    pub(crate) align: u8,
}

pub(crate) struct Parser {
    pub(crate) tokens: Vec<Token>,
    pub(crate) position: usize,
    /// Declared struct layouts, by tag name.
    pub(crate) structs: HashMap<String, StructLayout>,
    /// In-scope variables that are struct pointers, mapped to their struct tag,
    /// so `variable->field` resolves to the right layout.
    pub(crate) variable_structs: HashMap<String, String>,
    /// `typedef`-declared type aliases (e.g. `u32` -> `unsigned int`).
    pub(crate) typedefs: HashMap<String, Type>,
    /// Set by [`Parser::parse_type`] when it just parsed a `struct Name*`, so the
    /// declarator parser can associate the variable name with the struct tag.
    pub(crate) last_struct_tag: Option<String>,
    /// The struct tag of the expression `factor` just returned, so the tag survives
    /// a wrapping `(...)` — `((struct S *)x)->field` resolves through the parens.
    pub(crate) expression_struct_tag: Option<String>,
    /// Set by [`Parser::parse_type`] when the type carried a leading `const`. It is
    /// transparent on a parameter/local/return type, but on a file-scope global it
    /// changes the section (read-only) — so the global path defers when it is set.
    pub(crate) last_type_was_const: bool,
    /// Names of skipped `static inline` functions with an inline `asm {}` body, in
    /// declaration order; mwcc emits a local undefined symbol for each.
    pub(crate) inline_asm_symbols: Vec<String>,
    /// `typedef`-declared struct aliases (`typedef struct _FILE {…} FILE;`) mapped
    /// to their struct tag, so `FILE *p` resolves to the right layout.
    pub(crate) struct_typedefs: HashMap<String, String>,
    /// `typedef`-declared struct-POINTER aliases (`typedef struct {…} *VecPtr;`)
    /// mapped to their struct tag — the alias itself is already a struct pointer.
    pub(crate) struct_pointer_typedefs: HashMap<String, String>,
    /// Enumeration constant values, so a bare enumerator resolves to its integer
    /// value in an expression. (`-enum int`: an enum type is a 4-byte `int`.)
    pub(crate) enum_constants: HashMap<String, i64>,
}

impl Parser {
    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }
    /// The token `offset` positions ahead, clamped to the final (end-of-input)
    /// token so lookahead never runs off the end.
    pub(crate) fn peek_at(&self, offset: usize) -> &Token {
        let index = (self.position + offset).min(self.tokens.len() - 1);
        &self.tokens[index]
    }
    /// If the next two tokens are an arithmetic/bitwise operator followed by `=`
    /// (a compound assignment like `+=`), return the operator. The operator and
    /// `=` are NOT consumed.
    pub(crate) fn peek_compound_assignment(&self) -> Option<mwcc_syntax_trees::BinaryOperator> {
        use mwcc_syntax_trees::BinaryOperator;
        if *self.peek_at(1) != Token::Equals {
            return None;
        }
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
            _ => return None,
        })
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
