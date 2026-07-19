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
    /// Capture `template <typename T> struct Name { T a, b; ... };` at the
    /// current recovery position without advancing the main parser cursor.
    pub(crate) fn capture_skipped_struct_template(&mut self) {
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

    /// Instantiate `typedef Template<Concrete> Alias;` from a recovered
    /// template. Returns true only when the complete declaration was consumed
    /// conceptually; the caller's recovery scanner still advances the cursor.
    pub(crate) fn capture_skipped_template_typedef(&mut self) -> bool {
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
                size,
                align: alignment as u8,
            },
        );
        self.struct_typedefs.insert(alias.clone(), alias.clone());
        true
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
