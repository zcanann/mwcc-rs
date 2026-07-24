//! Source-proven iterator mutation facts.
//!
//! C++ overload syntax must not collapse into arithmetic merely because an
//! iterator occupies one word. This module admits only exact inline body
//! shapes and composes nested one-word wrappers with a pointer-backed
//! implementation iterator.

use mwcc_syntax_trees::{Expression, Type};
use mwcc_tokens::Token;

use crate::parser::{Parser, StructLayout};

#[derive(Clone, Copy)]
struct MethodBody<'a> {
    declaration: &'a [Token],
    body: &'a [Token],
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IteratorComparison {
    pub(crate) storage_offset: u32,
    pub(crate) supports_inequality: bool,
    terminal_type: Type,
    terminal_tag: Option<String>,
}

/// The runtime word constructed by an exact zero-argument iterator endpoint.
///
/// The source method still returns an iterator aggregate, but a proven
/// one-word wrapper can be represented by the pointer value that its inlined
/// constructor stores.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IteratorEndpoint {
    return_wrapper: String,
    value: IteratorEndpointValue,
}

#[derive(Clone, Debug, PartialEq)]
enum IteratorEndpointValue {
    LoadMember {
        offset: u32,
        member_type: Type,
    },
    AddressMember {
        offset: u32,
        member_type: Type,
    },
}

impl IteratorEndpoint {
    pub(crate) fn lower(&self, object: Expression) -> Expression {
        match self.value {
            IteratorEndpointValue::LoadMember {
                offset,
                member_type,
            } => Expression::Member {
                base: Box::new(object),
                offset,
                member_type,
                index_stride: None,
            },
            IteratorEndpointValue::AddressMember {
                offset,
                member_type,
            } => Expression::AddressOf {
                operand: Box::new(Expression::Member {
                    base: Box::new(object),
                    offset,
                    member_type,
                    index_stride: None,
                }),
            },
        }
    }
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
            if let (Some(name), Some(return_wrapper)) = (
                ordinary_zero_argument_method_name(method.declaration),
                zero_argument_method_return_wrapper(method.declaration),
            ) {
                let endpoint = match method.body {
                    [
                        Token::KeywordReturn,
                        Token::Identifier(wrapper),
                        Token::ParenOpen,
                        Token::Identifier(field),
                        Token::Dot,
                        Token::Identifier(accessor),
                        Token::ParenOpen,
                        Token::ParenClose,
                        Token::ParenClose,
                        Token::Semicolon,
                    ] if wrapper == return_wrapper => {
                        self.capture_begin_endpoint(layout, field, accessor, wrapper)
                    }
                    [
                        Token::KeywordReturn,
                        Token::Identifier(wrapper),
                        Token::ParenOpen,
                        Token::Ampersand,
                        Token::Identifier(field),
                        Token::ParenClose,
                        Token::Semicolon,
                    ] if wrapper == return_wrapper => {
                        self.capture_end_endpoint(layout, field, wrapper)
                    }
                    _ => None,
                };
                if let Some(endpoint) = endpoint {
                    self.source_iterator_endpoints
                        .insert((class.to_owned(), name.to_owned()), endpoint);
                }
            }

            if is_prefix_increment_declaration(method.declaration) {
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

            if is_equality_declaration(method.declaration) {
                if let [
                    Token::KeywordReturn,
                    Token::Identifier(left),
                    Token::Dot,
                    Token::Identifier(left_field),
                    Token::EqualEqual,
                    Token::Identifier(right),
                    Token::Dot,
                    Token::Identifier(right_field),
                    Token::Semicolon,
                ] = method.body
                {
                    if left != right && left_field == right_field {
                        if let Some(field) = layout.fields.get(left_field) {
                            if matches!(
                                field.member_type,
                                Type::Pointer(_)
                                    | Type::StructPointer { .. }
                                    | Type::Struct { size: 4, .. }
                            ) {
                                self.source_iterator_equality_fields
                                    .insert(class.to_owned(), field.offset);
                            }
                        }
                    }
                }
            }

            if is_inequality_declaration(method.declaration)
                && matches!(
                    method.body,
                    [
                        Token::KeywordReturn,
                        Token::Bang,
                        Token::ParenOpen,
                        Token::Identifier(left),
                        Token::EqualEqual,
                        Token::Identifier(right),
                        Token::ParenClose,
                        Token::Semicolon,
                    ] if left != right
                )
            {
                self.source_iterator_inequalities.insert(class.to_owned());
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

    /// Compose an exact primary-template wrapper with the exact base endpoint
    /// it forwards. The concrete wrapper must resolve to one pointer word at
    /// offset zero; wider or rearranged aggregates remain ordinary calls.
    pub(crate) fn resolve_inline_iterator_endpoint(
        &self,
        class: &str,
        member: &str,
    ) -> Option<(IteratorEndpoint, String, Type)> {
        let (base, base_member, wrapper) =
            self.resolve_inline_template_base_forwarder(class, member, 0)?;
        let endpoint = self.resolve_source_iterator_endpoint(&base, &base_member)?;
        if endpoint.return_wrapper != wrapper {
            return None;
        }
        let concrete = format!("{class}::{wrapper}");
        let comparison = self.resolve_iterator_pointer_comparison(&concrete)?;
        if comparison.storage_offset != 0 {
            return None;
        }
        let layout = self
            .structs
            .get(&concrete)
            .or_else(|| {
                self.resolve_nested_template_alias_layout(&concrete)
                    .and_then(|(generic, _)| self.structs.get(&generic))
            })?;
        if layout.size != 4 {
            return None;
        }
        Some((
            endpoint,
            concrete,
            Type::Struct {
                size: layout.size,
                align: layout.align,
            },
        ))
    }

    fn resolve_source_iterator_endpoint(
        &self,
        class: &str,
        member: &str,
    ) -> Option<IteratorEndpoint> {
        if let Some(endpoint) = self
            .source_iterator_endpoints
            .get(&(class.to_owned(), member.to_owned()))
        {
            return Some(endpoint.clone());
        }
        let terminal = class.rsplit("::").next().unwrap_or(class);
        let mut matches = self
            .source_iterator_endpoints
            .iter()
            .filter(|((owner, candidate), _)| {
                candidate == member && owner.rsplit("::").next() == Some(terminal)
            });
        let (_, endpoint) = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(endpoint.clone())
    }

    fn capture_begin_endpoint(
        &self,
        layout: &StructLayout,
        field: &str,
        accessor: &str,
        return_wrapper: &str,
    ) -> Option<IteratorEndpoint> {
        let field = layout.fields.get(field)?;
        if !matches!(field.member_type, Type::Struct { .. }) {
            return None;
        }
        let pointee = field.struct_tag.as_deref()?;
        let accessor_offset = self
            .source_pointer_accessors
            .get(&(pointee.to_owned(), accessor.to_owned()))
            .copied()?;
        let pointee_layout = self.structs.get(pointee)?;
        let mut candidates = pointee_layout.fields.values().filter(|candidate| {
            candidate.offset == accessor_offset
                && candidate.array_bytes.is_none()
                && candidate.bit_field.is_none()
                && matches!(
                    candidate.member_type,
                    Type::Pointer(_) | Type::StructPointer { .. }
                )
        });
        let storage = candidates.next()?;
        if candidates.next().is_some() {
            return None;
        }
        Some(IteratorEndpoint {
            return_wrapper: return_wrapper.to_owned(),
            value: IteratorEndpointValue::LoadMember {
                offset: field.offset.checked_add(accessor_offset)?,
                member_type: storage.member_type,
            },
        })
    }

    fn capture_end_endpoint(
        &self,
        layout: &StructLayout,
        field: &str,
        return_wrapper: &str,
    ) -> Option<IteratorEndpoint> {
        let field = layout.fields.get(field)?;
        if !matches!(field.member_type, Type::Struct { .. }) || field.array_bytes.is_some() {
            return None;
        }
        Some(IteratorEndpoint {
            return_wrapper: return_wrapper.to_owned(),
            value: IteratorEndpointValue::AddressMember {
                offset: field.offset,
                member_type: field.member_type,
            },
        })
    }

    /// Resolve an equality/inequality overload to the pointer storage it
    /// compares. Equality fields may themselves be one-word iterator wrappers;
    /// every layer must carry the exact friend comparison body.
    pub(crate) fn resolve_iterator_pointer_comparison(
        &self,
        iterator: &str,
    ) -> Option<IteratorComparison> {
        self.concrete_template_iterator_comparisons
            .get(iterator)
            .cloned()
            .or_else(|| {
                let (generic, concrete) =
                    self.resolve_nested_template_alias_layout(iterator)?;
                self.concrete_template_iterator_comparisons
                    .get(&concrete)
                    .cloned()
                    .or_else(|| {
                        self.resolve_source_iterator_pointer_comparison_inner(
                            &generic,
                            &mut std::collections::HashSet::new(),
                        )
                    })
            })
            .or_else(|| {
                self.resolve_source_iterator_pointer_comparison_inner(
                    iterator,
                    &mut std::collections::HashSet::new(),
                )
            })
    }

    fn resolve_source_iterator_pointer_comparison_inner(
        &self,
        iterator: &str,
        visiting: &mut std::collections::HashSet<String>,
    ) -> Option<IteratorComparison> {
        if !visiting.insert(iterator.to_owned()) {
            return None;
        }
        let field_offset = *self.source_iterator_equality_fields.get(iterator)?;
        let layout = self.structs.get(iterator)?;
        if layout.size != 4 {
            return None;
        }
        let mut fields = layout.fields.values().filter(|field| {
            field.array_bytes.is_none()
                && field.bit_field.is_none()
                && field.offset == field_offset
        });
        let field = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        let supports_inequality = self.source_iterator_inequalities.contains(iterator);
        match field.member_type {
            Type::Pointer(_) | Type::StructPointer { .. } => {
                Some(IteratorComparison {
                    storage_offset: field_offset,
                    supports_inequality,
                    terminal_type: field.member_type,
                    terminal_tag: field.struct_tag.clone(),
                })
            }
            Type::Struct { size: 4, .. } => {
                let nested = field.struct_tag.as_deref()?;
                let nested = self.resolve_iterator_semantic_identity(nested)?;
                let nested = self
                    .resolve_source_iterator_pointer_comparison_inner(&nested, visiting)?;
                Some(IteratorComparison {
                    storage_offset: field_offset.checked_add(nested.storage_offset)?,
                    supports_inequality,
                    terminal_type: nested.terminal_type,
                    terminal_tag: nested.terminal_tag,
                })
            }
            _ => None,
        }
    }

    pub(crate) fn iterator_comparisons_are_compatible(
        left: &IteratorComparison,
        right: &IteratorComparison,
    ) -> bool {
        left.terminal_type == right.terminal_type && left.terminal_tag == right.terminal_tag
    }

    fn resolve_iterator_semantic_identity(&self, source: &str) -> Option<String> {
        if self.source_iterator_pointer_steps.contains_key(source)
            || self.source_iterator_step_forwarders.contains_key(source)
            || self.source_iterator_equality_fields.contains_key(source)
        {
            return Some(source.to_owned());
        }
        if let Some(resolved) = self.struct_typedefs.get(source) {
            if self.source_iterator_pointer_steps.contains_key(resolved)
                || self.source_iterator_step_forwarders.contains_key(resolved)
                || self.source_iterator_equality_fields.contains_key(resolved)
            {
                return Some(resolved.clone());
            }
        }
        let suffix = format!("::{source}");
        let mut candidates = self
            .source_iterator_pointer_steps
            .keys()
            .chain(self.source_iterator_step_forwarders.keys())
            .chain(self.source_iterator_equality_fields.keys())
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

fn zero_argument_method_return_wrapper(tokens: &[Token]) -> Option<&str> {
    tokens.windows(4).rev().find_map(|window| match window {
        [
            Token::Identifier(wrapper),
            Token::Identifier(name),
            Token::ParenOpen,
            Token::ParenClose,
        ] if name != "operator" => Some(wrapper.as_str()),
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

fn is_equality_declaration(tokens: &[Token]) -> bool {
    operator_declaration(tokens, Token::EqualEqual)
}

fn is_inequality_declaration(tokens: &[Token]) -> bool {
    operator_declaration(tokens, Token::BangEqual)
}

fn operator_declaration(tokens: &[Token], punctuation: Token) -> bool {
    tokens.windows(3).any(|window| {
        matches!(
            window,
            [Token::Identifier(operator), token, Token::ParenOpen]
                if operator == "operator" && *token == punctuation
        )
    })
}
