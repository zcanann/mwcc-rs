//! Source-proven iterator mutation facts.
//!
//! C++ overload syntax must not collapse into arithmetic merely because an
//! iterator occupies one word. This module admits only exact inline body
//! shapes and composes nested one-word wrappers with a pointer-backed
//! implementation iterator.

use mwcc_syntax_trees::Type;
use mwcc_tokens::Token;

use crate::parser::{Parser, StructLayout};

#[derive(Clone, Copy)]
struct MethodBody<'a> {
    declaration: &'a [Token],
    body: &'a [Token],
}

impl Parser {
    /// Capture exact iterator/accessor semantics after a class layout is
    /// complete. Nested class bodies are parsed independently and skipped by
    /// the top-level method-body walk.
    pub(crate) fn capture_iterator_class_semantics(
        &mut self,
        class: &str,
        layout: &StructLayout,
        body_start: usize,
        body_end: usize,
    ) {
        let methods = top_level_method_bodies(&self.tokens[body_start..body_end]);

        // Accessors are collected first so declaration order within one class
        // cannot affect whether a later step body is provable.
        for method in &methods {
            let Some(name) = ordinary_zero_argument_method_name(method.declaration) else {
                continue;
            };
            let [Token::KeywordReturn, Token::Identifier(field), Token::Semicolon] = method.body
            else {
                continue;
            };
            let Some(field) = layout.fields.get(field) else {
                continue;
            };
            if matches!(
                field.member_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            ) {
                self.source_pointer_accessors
                    .insert((class.to_owned(), name.to_owned()), field.offset);
            }
        }

        for method in methods {
            if !is_prefix_increment_declaration(method.declaration) {
                continue;
            }
            match method.body {
                [
                    Token::Identifier(target),
                    Token::Equals,
                    Token::Identifier(source),
                    Token::Arrow,
                    Token::Identifier(accessor),
                    Token::ParenOpen,
                    Token::ParenClose,
                    Token::Semicolon,
                    Token::KeywordReturn,
                    Token::Star,
                    Token::Identifier(this),
                    Token::Semicolon,
                ] if target == source && this == "this" => {
                    let Some(storage) = layout.fields.get(target) else {
                        continue;
                    };
                    if !matches!(
                        storage.member_type,
                        Type::Pointer(_) | Type::StructPointer { .. }
                    ) {
                        continue;
                    }
                    let Some(pointee) = storage.struct_tag.as_deref() else {
                        continue;
                    };
                    let Some(next_offset) = self
                        .source_pointer_accessors
                        .get(&(pointee.to_owned(), accessor.to_owned()))
                        .copied()
                    else {
                        continue;
                    };
                    self.source_iterator_pointer_steps
                        .insert(class.to_owned(), (storage.offset, next_offset));
                }
                [
                    Token::PlusPlus,
                    Token::Identifier(field),
                    Token::Semicolon,
                    Token::KeywordReturn,
                    Token::Star,
                    Token::Identifier(this),
                    Token::Semicolon,
                ] if this == "this" => {
                    let Some(field) = layout.fields.get(field) else {
                        continue;
                    };
                    if matches!(field.member_type, Type::Struct { size: 4, .. })
                        && field.struct_tag.is_some()
                    {
                        self.source_iterator_step_forwarders
                            .insert(class.to_owned(), field.offset);
                    }
                }
                _ => {}
            }
        }
    }

    /// Resolve a direct pointer iterator or a chain of source-proven one-word
    /// forwarding wrappers.
    pub(crate) fn resolve_source_iterator_pointer_step(
        &self,
        iterator: &str,
    ) -> Option<(u32, u32)> {
        self.resolve_source_iterator_pointer_step_inner(
            iterator,
            &mut std::collections::HashSet::new(),
        )
    }

    fn resolve_source_iterator_pointer_step_inner(
        &self,
        iterator: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> Option<(u32, u32)> {
        if !visiting.insert(iterator.to_owned()) {
            return None;
        }
        if let Some(step) = self.source_iterator_pointer_steps.get(iterator) {
            return Some(*step);
        }
        let wrapper_offset = *self.source_iterator_step_forwarders.get(iterator)?;
        let layout = self.structs.get(iterator)?;
        if layout.size != 4 {
            return None;
        }
        let mut fields = layout.fields.values().filter(|field| {
            field.array_bytes.is_none()
                && field.bit_field.is_none()
                && field.offset == wrapper_offset
        });
        let field = fields.next()?;
        if fields.next().is_some() || !matches!(field.member_type, Type::Struct { size: 4, .. }) {
            return None;
        }
        let nested = field.struct_tag.as_deref()?;
        let nested = self.resolve_iterator_semantic_identity(nested)?;
        let (storage_offset, next_offset) =
            self.resolve_source_iterator_pointer_step_inner(&nested, visiting)?;
        Some((
            wrapper_offset.checked_add(storage_offset)?,
            next_offset,
        ))
    }

    pub(crate) fn resolve_concrete_template_iterator_step(
        &self,
        iterator: &str,
    ) -> Option<(u32, u32)> {
        self.concrete_template_iterator_steps
            .get(iterator)
            .copied()
            .or_else(|| self.resolve_source_iterator_pointer_step(iterator))
    }

    fn resolve_iterator_semantic_identity(&self, source: &str) -> Option<String> {
        if self.source_iterator_pointer_steps.contains_key(source)
            || self.source_iterator_step_forwarders.contains_key(source)
        {
            return Some(source.to_owned());
        }
        if let Some(resolved) = self.struct_typedefs.get(source) {
            if self.source_iterator_pointer_steps.contains_key(resolved)
                || self.source_iterator_step_forwarders.contains_key(resolved)
            {
                return Some(resolved.clone());
            }
        }
        let suffix = format!("::{source}");
        let mut candidates = self
            .source_iterator_pointer_steps
            .keys()
            .chain(self.source_iterator_step_forwarders.keys())
            .filter(|candidate| candidate.ends_with(&suffix));
        let candidate = candidates.next()?.clone();
        if candidates.next().is_some() {
            return None;
        }
        Some(candidate)
    }
}

fn top_level_method_bodies(tokens: &[Token]) -> Vec<MethodBody<'_>> {
    let mut methods = Vec::new();
    let mut member_start = 0usize;
    let mut cursor = 0usize;
    while cursor < tokens.len() {
        match tokens.get(cursor) {
            Some(Token::Semicolon) => {
                member_start = cursor + 1;
                cursor += 1;
            }
            Some(Token::BraceOpen) => {
                let Some(close) = matching_brace(tokens, cursor) else {
                    break;
                };
                let declaration = &tokens[member_start..cursor];
                if declaration.iter().any(|token| *token == Token::ParenClose) {
                    methods.push(MethodBody {
                        declaration,
                        body: &tokens[cursor + 1..close],
                    });
                }
                cursor = close + 1;
                if tokens.get(cursor) == Some(&Token::Semicolon) {
                    cursor += 1;
                }
                member_start = cursor;
            }
            _ => cursor += 1,
        }
    }
    methods
}

fn matching_brace(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut cursor = open + 1;
    while cursor < tokens.len() {
        match tokens.get(cursor) {
            Some(Token::BraceOpen) => depth += 1,
            Some(Token::BraceClose) => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn ordinary_zero_argument_method_name(tokens: &[Token]) -> Option<&str> {
    tokens.windows(3).rev().find_map(|window| match window {
        [Token::Identifier(name), Token::ParenOpen, Token::ParenClose]
            if name != "operator" =>
        {
            Some(name.as_str())
        }
        _ => None,
    })
}

fn is_prefix_increment_declaration(tokens: &[Token]) -> bool {
    tokens.windows(4).any(|window| {
        matches!(
            window,
            [
                Token::Identifier(operator),
                Token::PlusPlus,
                Token::ParenOpen,
                Token::ParenClose
            ] if operator == "operator"
        )
    })
}
