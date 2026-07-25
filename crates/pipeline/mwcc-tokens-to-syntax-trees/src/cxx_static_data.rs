//! Declaration recovery for C++ class-static data members.
//!
//! Storage belongs to an out-of-class definition, but every member body sees
//! the complete class scope. This pass therefore runs before in-class inline
//! bodies are parsed and exposes scalar static declarations as extern globals.

use mwcc_syntax_trees::{Pointee, Type};
use mwcc_tokens::Token;

use crate::cxx::mangle_qualified_data_member;
use crate::items::{pointee_of, type_size};
use crate::parser::Parser;

impl Parser {
    /// Record ordinary scalar static-data declarations from one complete class
    /// body. Function declarations contain parentheses and arrays need separate
    /// extent recovery, so both are conservatively excluded.
    pub(crate) fn capture_cxx_static_data_members(&mut self, body_start: usize, class: &str) {
        let mut index = body_start;
        let mut brace_depth = 1i32;
        while index < self.tokens.len() && brace_depth > 0 {
            match self.tokens.get(index) {
                Some(Token::BraceOpen) => brace_depth += 1,
                Some(Token::BraceClose) => brace_depth -= 1,
                Some(Token::Identifier(storage))
                    if brace_depth == 1 && storage == "static" =>
                {
                    let declaration_start = index + 1;
                    let mut end = declaration_start;
                    let mut parentheses = 0i32;
                    let mut brackets = 0i32;
                    while end < self.tokens.len() {
                        match self.tokens.get(end) {
                            Some(Token::ParenOpen) => parentheses += 1,
                            Some(Token::ParenClose) => parentheses -= 1,
                            Some(Token::BracketOpen) => brackets += 1,
                            Some(Token::BracketClose) => brackets -= 1,
                            Some(Token::BraceOpen)
                                if parentheses == 0 && brackets == 0 =>
                            {
                                break;
                            }
                            Some(Token::Semicolon)
                                if parentheses == 0 && brackets == 0 =>
                            {
                                break;
                            }
                            Some(Token::EndOfFile) | None => break,
                            _ => {}
                        }
                        end += 1;
                    }
                    if self.tokens.get(end) == Some(&Token::Semicolon)
                        && !self.tokens[declaration_start..end]
                            .iter()
                            .any(|token| {
                                matches!(token, Token::ParenOpen | Token::BracketOpen)
                            })
                    {
                        self.record_scalar_static_declarators(
                            declaration_start,
                            end,
                            class,
                        );
                    }
                }
                Some(Token::EndOfFile) | None => break,
                _ => {}
            }
            index += 1;
        }
    }

    fn record_scalar_static_declarators(
        &mut self,
        declaration_start: usize,
        end: usize,
        class: &str,
    ) {
        let Some((declaration_type, declaration_struct)) =
            self.static_data_declaration_type(declaration_start)
        else {
            return;
        };
        let mut declarator_start = declaration_start;
        for declarator_end in (declaration_start..=end).filter(|&cursor| {
            cursor == end || self.tokens.get(cursor) == Some(&Token::Comma)
        }) {
            let name_end = (declarator_start..declarator_end)
                .find(|&cursor| {
                    matches!(
                        self.tokens.get(cursor),
                        Some(Token::BracketOpen | Token::Equals)
                    )
                })
                .unwrap_or(declarator_end);
            if let Some(name) = self.tokens[declarator_start..name_end]
                .iter()
                .rev()
                .find_map(|token| match token {
                    Token::Identifier(name)
                        if !matches!(name.as_str(), "const" | "volatile" | "mutable") =>
                    {
                        Some(name.clone())
                    }
                    _ => None,
                })
            {
                self.cxx_static_data_members
                    .insert((class.to_owned(), name.clone()), declaration_type);
                if let Ok(mangled) = mangle_qualified_data_member(
                    &class.split("::").collect::<Vec<_>>(),
                    &name,
                ) {
                    let size = type_size(declaration_type);
                    self.global_sizes.entry(mangled.clone()).or_insert((size, None));
                    self.global_types
                        .entry(mangled.clone())
                        .or_insert(declaration_type);
                    if let Some(tag) = &declaration_struct {
                        self.global_structs
                            .entry(mangled)
                            .or_insert_with(|| tag.clone());
                    }
                }
            }
            declarator_start = declarator_end + 1;
        }
    }

    fn static_data_declaration_type(
        &self,
        declaration_start: usize,
    ) -> Option<(Type, Option<String>)> {
        let mut probe = self.clone();
        probe.position = declaration_start;
        let mut declared_type = probe.parse_type().ok()?;
        let declaration_struct = probe.last_struct_tag.take();
        while probe.tokens.get(probe.position) == Some(&Token::Star) {
            probe.position += 1;
            declared_type = match declared_type {
                Type::Struct { size, .. } => Type::StructPointer { element_size: size },
                Type::Pointer(_) | Type::StructPointer { .. } => {
                    Type::Pointer(Pointee::Pointer)
                }
                scalar => Type::Pointer(pointee_of(scalar).ok()?),
            };
        }
        Some((declared_type, declaration_struct))
    }
}
