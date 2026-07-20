//! Layout recovery for skipped single-parameter C++ struct templates.
//!
//! The general C++ parser does not yet compile template definitions. We still
//! need their concrete instance layout when later non-template functions use a
//! typedef such as `Vector3<float>`. This module records only parameter-typed
//! instance fields; methods, nested bodies, and static members remain skipped.

use super::{type_alignment, type_size};
use crate::parser::{Parser, StructField, StructLayout, StructTemplate};
use mwcc_syntax_trees::Type;
use mwcc_tokens::Token;
use std::collections::HashMap;

impl Parser {
    /// A generic primary template (`template <typename T, ...>`), as opposed
    /// to an explicit specialization (`template <>`). Primary definitions do
    /// not emit code or data until instantiated, so recovery may skip them.
    pub(crate) fn item_is_primary_template_declaration(&self) -> bool {
        matches!(self.tokens.get(self.position), Some(Token::Identifier(word)) if word == "template")
            && self.tokens.get(self.position + 1) == Some(&Token::Less)
            && self.tokens.get(self.position + 2) != Some(&Token::Greater)
    }

    /// Recognize an out-of-class definition proven to retain inline semantics.
    /// This covers both a concrete template member (`T Table<8, T>::get(...)`)
    /// and an ordinary member whose earlier class declaration said `inline`.
    ///
    /// CodeWarrior treats these header specializations like inline template
    /// materializations: an unused definition emits no function. Recovery can
    /// therefore skip it, while the ordinary skipped-inline name tracking makes
    /// a later call defer until template instantiation is implemented.
    pub(crate) fn item_is_skippable_inline_member_definition(&self) -> bool {
        let explicit_specialization = matches!(
            self.tokens.get(self.position..self.position + 3),
            Some([Token::Identifier(template), Token::Less, Token::Greater]) if template == "template"
        );
        let mut index = self.position + if explicit_specialization { 3 } else { 0 };
        let mut angle_depth = 0i32;
        let mut parameter_depth = 0i32;
        let mut saw_template_arguments = false;
        let mut saw_qualified_member = false;
        let mut saw_parameter_list = false;
        let mut last_identifier: Option<&str> = None;
        let mut angle_qualified_name: Option<&str> = None;
        let mut class_name: Option<&str> = None;
        let mut member_name: Option<&str> = None;
        let mut awaiting_member = false;

        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Less if parameter_depth == 0 => {
                    if angle_depth == 0 {
                        angle_qualified_name = last_identifier;
                    }
                    angle_depth += 1;
                    saw_template_arguments = true;
                }
                Token::Greater if parameter_depth == 0 && angle_depth > 0 => angle_depth -= 1,
                Token::Colon
                    if parameter_depth == 0
                        && angle_depth == 0
                        && self.tokens.get(index + 1) == Some(&Token::Colon) =>
                {
                    // Keep the final qualifier/member pair. This naturally
                    // handles `N::C::f`: the second `::` replaces `N` with `C`.
                    class_name = angle_qualified_name.take().or(last_identifier);
                    member_name = None;
                    saw_qualified_member = true;
                    awaiting_member = true;
                    index += 1;
                }
                Token::Identifier(name) if parameter_depth == 0 && angle_depth == 0 => {
                    if awaiting_member && !saw_parameter_list {
                        member_name = Some(name);
                        awaiting_member = false;
                    }
                    last_identifier = Some(name);
                }
                Token::ParenOpen if angle_depth == 0 => parameter_depth += 1,
                Token::ParenClose if parameter_depth > 0 => {
                    parameter_depth -= 1;
                    if parameter_depth == 0 {
                        saw_parameter_list = true;
                    }
                }
                Token::BraceOpen if angle_depth == 0 && parameter_depth == 0 => {
                    if !(saw_qualified_member && saw_parameter_list) {
                        return false;
                    }
                    let Some((class, member)) = class_name.zip(member_name) else {
                        return false;
                    };
                    let qualified_class = self.qualify_cxx_class_name(class);
                    let ordinary_inline = self
                        .inline_cxx_members
                        .contains(&(qualified_class, member.to_string()));
                    let template_inline = (explicit_specialization
                        || saw_template_arguments
                        || self.template_aliases.contains_key(class))
                        && {
                            let primary = self
                                .template_aliases
                                .get(class)
                                .map_or(class, String::as_str);
                            self.inline_template_members
                                .contains(&(primary.to_string(), member.to_string()))
                        };
                    return ordinary_inline || template_inline;
                }
                Token::Semicolon if angle_depth == 0 && parameter_depth == 0 => return false,
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    /// Capture `template <typename T> struct Name { T a, b; ... };` at the
    /// current recovery position without advancing the main parser cursor.
    pub(crate) fn capture_skipped_struct_template(&mut self) {
        self.capture_inline_template_members();
        let start = self.position;
        let header = self.tokens.get(start..start + 8);
        let Some(
            [Token::Identifier(template), Token::Less, Token::Identifier(parameter_kind), Token::Identifier(parameter), Token::Greater, struct_or_class, Token::Identifier(name), Token::BraceOpen],
        ) = header
        else {
            return;
        };
        let is_struct_or_class = *struct_or_class == Token::KeywordStruct
            || matches!(struct_or_class, Token::Identifier(word) if word == "class");
        if template != "template"
            || !matches!(parameter_kind.as_str(), "typename" | "class")
            || !is_struct_or_class
        {
            return;
        }

        let mut fields = Vec::new();
        let mut index = start + 8;
        let mut brace_depth = 1i32;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => {
                    brace_depth += 1;
                    index += 1;
                }
                Token::BraceClose => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        break;
                    }
                    index += 1;
                }
                Token::Identifier(word) if brace_depth == 1 && word == parameter => {
                    let mut candidate_fields = Vec::new();
                    let mut cursor = index + 1;
                    let mut expect_name = true;
                    let mut valid = true;
                    while let Some(candidate) = self.tokens.get(cursor) {
                        match candidate {
                            Token::Identifier(field) if expect_name => {
                                candidate_fields.push(field.clone());
                                expect_name = false;
                            }
                            Token::Comma if !expect_name => expect_name = true,
                            Token::Semicolon => {
                                if valid && !expect_name && !candidate_fields.is_empty() {
                                    fields.extend(candidate_fields);
                                }
                                cursor += 1;
                                break;
                            }
                            Token::BraceOpen | Token::BraceClose | Token::EndOfFile => break,
                            _ => valid = false,
                        }
                        cursor += 1;
                    }
                    index = cursor;
                }
                Token::EndOfFile => break,
                _ => index += 1,
            }
        }
        if !fields.is_empty() {
            self.struct_templates
                .insert(name.clone(), StructTemplate { fields });
        }
    }

    /// Record methods defined directly inside any class-template body. This is
    /// deliberately independent of layout recovery, which currently supports
    /// only one type parameter; Pikmin's trig templates use both an integer and
    /// a type parameter but still need correct specialization materialization.
    fn capture_inline_template_members(&mut self) {
        let start = self.position;
        if !self.item_is_primary_template_declaration() {
            return;
        }
        let mut index = start + 1;
        let mut angle_depth = 0i32;
        loop {
            match self.tokens.get(index) {
                Some(Token::Less) => angle_depth += 1,
                Some(Token::Greater) => {
                    angle_depth -= 1;
                    if angle_depth == 0 {
                        index += 1;
                        break;
                    }
                }
                Some(Token::EndOfFile) | None => return,
                _ => {}
            }
            index += 1;
        }
        let is_class = matches!(self.tokens.get(index), Some(Token::KeywordStruct))
            || matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "class");
        if !is_class {
            return;
        }
        index += 1;
        let Some(Token::Identifier(class_name)) = self.tokens.get(index) else {
            return;
        };
        let class_name = class_name.clone();
        while !matches!(
            self.tokens.get(index),
            Some(Token::BraceOpen | Token::EndOfFile) | None
        ) {
            index += 1;
        }
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return;
        }
        index += 1;
        let mut brace_depth = 1i32;
        while let Some(token) = self.tokens.get(index) {
            if brace_depth == 1 {
                if let Token::Identifier(member_name) = token {
                    if self.tokens.get(index + 1) == Some(&Token::ParenOpen) {
                        let mut cursor = index + 1;
                        let mut parens = 0i32;
                        while let Some(candidate) = self.tokens.get(cursor) {
                            match candidate {
                                Token::ParenOpen => parens += 1,
                                Token::ParenClose => {
                                    parens -= 1;
                                    if parens == 0 {
                                        cursor += 1;
                                        break;
                                    }
                                }
                                Token::EndOfFile => return,
                                _ => {}
                            }
                            cursor += 1;
                        }
                        while matches!(self.tokens.get(cursor), Some(Token::Identifier(_))) {
                            cursor += 1;
                        }
                        if self.tokens.get(cursor) == Some(&Token::BraceOpen) {
                            self.inline_template_members
                                .insert((class_name.clone(), member_name.clone()));
                        }
                    }
                }
            }
            match token {
                Token::BraceOpen => brace_depth += 1,
                Token::BraceClose => {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        return;
                    }
                }
                Token::EndOfFile => return,
                _ => {}
            }
            index += 1;
        }
    }

    /// Instantiate `typedef Template<Concrete> Alias;` from a recovered
    /// template. Returns true only when the complete declaration was consumed
    /// conceptually; the caller's recovery scanner still advances the cursor.
    pub(crate) fn capture_skipped_template_typedef(&mut self) -> bool {
        self.capture_template_alias();
        let start = self.position;
        let Some(
            [Token::Identifier(typedef), Token::Identifier(template_name), Token::Less, argument_token, Token::Greater, Token::Identifier(alias), Token::Semicolon],
        ) = self.tokens.get(start..start + 7)
        else {
            return false;
        };
        if typedef != "typedef" {
            return false;
        }
        let Some(template) = self.struct_templates.get(template_name) else {
            return false;
        };
        let Some(argument) = self.template_argument_type(argument_token) else {
            return false;
        };
        let alignment = type_alignment(argument).max(1);
        let field_size = type_size(argument);
        let mut offset = 0u16;
        let mut fields = HashMap::new();
        for name in &template.fields {
            offset = offset.div_ceil(alignment) * alignment;
            fields.insert(
                name.clone(),
                StructField {
                    member_type: argument,
                    offset,
                    struct_tag: None,
                    array_element: None,
                    array_bytes: None,
                    bit_field: None,
                },
            );
            offset += field_size;
        }
        let size = offset.div_ceil(alignment) * alignment;
        self.structs.insert(
            alias.clone(),
            StructLayout {
                fields,
                function_pointer_fields: std::collections::HashSet::new(),
                size,
                align: alignment as u8,
            },
        );
        self.struct_typedefs.insert(alias.clone(), alias.clone());
        true
    }

    /// Capture `typedef [Scope::]Template<...> Alias;` even when the concrete
    /// argument list is too complex for layout recovery. The immediate name
    /// before the outer `<` is the primary template; the final top-level name
    /// after its matching `>` is the alias.
    fn capture_template_alias(&mut self) {
        let start = self.position;
        if !matches!(self.tokens.get(start), Some(Token::Identifier(word)) if word == "typedef") {
            return;
        }
        let mut index = start + 1;
        let mut previous_identifier: Option<String> = None;
        let mut primary: Option<String> = None;
        let mut angle_depth = 0i32;
        let mut closed_outer = false;
        let mut alias: Option<String> = None;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(name) if angle_depth == 0 => {
                    if closed_outer {
                        alias = Some(name.clone());
                    }
                    previous_identifier = Some(name.clone());
                }
                Token::Less => {
                    if angle_depth == 0 && primary.is_none() {
                        primary = previous_identifier.clone();
                    }
                    angle_depth += 1;
                }
                Token::Greater if angle_depth > 0 => {
                    angle_depth -= 1;
                    if angle_depth == 0 {
                        closed_outer = true;
                    }
                }
                Token::Semicolon if angle_depth == 0 => break,
                Token::EndOfFile => return,
                _ => {}
            }
            index += 1;
        }
        if let (Some(primary), Some(alias)) = (primary, alias) {
            self.template_aliases.insert(alias, primary);
        }
    }

    fn template_argument_type(&self, token: &Token) -> Option<Type> {
        match token {
            Token::KeywordInt => Some(Type::Int),
            Token::KeywordChar => Some(Type::Char),
            Token::KeywordShort => Some(Type::Short),
            Token::KeywordUnsigned => Some(Type::UnsignedInt),
            Token::KeywordFloat => Some(Type::Float),
            Token::Identifier(name) => self.typedefs.get(name).copied(),
            _ => None,
        }
    }
}
