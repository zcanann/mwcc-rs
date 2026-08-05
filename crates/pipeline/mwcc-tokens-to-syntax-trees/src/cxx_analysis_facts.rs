//! Syntax-only facts consumed by version-specific C++ analysis timelines.
//!
//! These scans must not depend on recoverable object layout: large preprocessed
//! headers can exceed the frontend's current layout subset while still changing
//! MWCC's anonymous-symbol counter.

use std::collections::{HashMap, HashSet};

use mwcc_tokens::Token;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParameterMemberKind {
    Scalar(String),
    String,
}

/// `Parm<T>` fields declared directly in one class body. Keeping this syntax
/// inventory separate from layout recovery lets incomplete project headers
/// still contribute to MWCC's discarded-inline analysis timeline.
#[derive(Debug, Clone, Default)]
pub(crate) struct ParameterMemberFields(HashMap<String, ParameterMemberKind>);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParameterInitializerFacts {
    pub scalar_members: usize,
    pub string_members: usize,
    pub heterogeneous_scalar_types: bool,
}

fn parameter_scalar_type_fingerprint(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| match token {
            Token::KeywordFloat => "float".to_string(),
            Token::KeywordInt => "int".to_string(),
            Token::KeywordChar => "char".to_string(),
            Token::KeywordShort => "short".to_string(),
            Token::KeywordUnsigned => "unsigned".to_string(),
            Token::Identifier(name) => match name.as_str() {
                "f32" => "float".to_string(),
                "f64" => "double".to_string(),
                "s8" => "signed char".to_string(),
                "u8" => "unsigned char".to_string(),
                "s16" => "short".to_string(),
                "u16" => "unsigned short".to_string(),
                "s32" => "int".to_string(),
                "u32" => "unsigned int".to_string(),
                _ => name.clone(),
            },
            _ => format!("{token:?}"),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Discover direct `Parm<T>` data members without entering nested classes or
/// inline method bodies. The executable class layout may be unrecoverable; the
/// token shape remains sufficient for source-analysis accounting.
pub(crate) fn parameter_member_fields(tokens: &[Token]) -> ParameterMemberFields {
    let mut fields = HashMap::new();
    let mut braces = 0usize;
    let mut index = 0usize;
    while let Some(token) = tokens.get(index) {
        match token {
            Token::BraceOpen => {
                braces += 1;
                index += 1;
                continue;
            }
            Token::BraceClose if braces == 0 => break,
            Token::BraceClose => {
                braces -= 1;
                index += 1;
                continue;
            }
            Token::Identifier(name)
                if braces == 0
                    && name == "Parm"
                    && tokens.get(index + 1) == Some(&Token::Less) => {}
            _ => {
                index += 1;
                continue;
            }
        }

        let mut template_depth = 1usize;
        let mut cursor = index + 2;
        let type_start = cursor;
        let mut string = false;
        while template_depth > 0 {
            match tokens.get(cursor) {
                Some(Token::Less) => template_depth += 1,
                Some(Token::Greater) => template_depth -= 1,
                Some(Token::Identifier(name)) if name.rsplit("::").next() == Some("String") => {
                    string = true;
                }
                Some(Token::EndOfFile) | None => return ParameterMemberFields(fields),
                _ => {}
            }
            cursor += 1;
        }
        let kind = if string {
            ParameterMemberKind::String
        } else {
            ParameterMemberKind::Scalar(parameter_scalar_type_fingerprint(
                &tokens[type_start..cursor - 1],
            ))
        };
        let Some(Token::Identifier(field)) = tokens.get(cursor) else {
            index = cursor;
            continue;
        };
        // `Parm<T> make(...)` is a method, not a data member.
        if tokens.get(cursor + 1) != Some(&Token::ParenOpen) {
            fields.insert(field.clone(), kind);
        }
        index = cursor + 1;
    }
    ParameterMemberFields(fields)
}

/// Count `Parm<T>` targets in an in-class constructor initializer list. Values
/// are skipped as balanced token groups so nested `String(...)` calls cannot be
/// mistaken for additional member initializers.
pub(crate) fn constructor_parameter_initializers(
    declaration: &[Token],
    fields: &ParameterMemberFields,
) -> ParameterInitializerFacts {
    let mut parens = 0usize;
    let Some(mut index) = declaration.iter().position(|token| {
        match token {
            Token::ParenOpen => parens += 1,
            Token::ParenClose => parens = parens.saturating_sub(1),
            Token::Colon if parens == 0 => return true,
            _ => {}
        }
        false
    }) else {
        return ParameterInitializerFacts::default();
    };
    index += 1;
    let mut facts = ParameterInitializerFacts::default();
    let mut counted = HashSet::new();
    let mut scalar_types = HashSet::new();
    while index < declaration.len() {
        let mut target = None;
        while index < declaration.len()
            && !matches!(declaration[index], Token::ParenOpen | Token::BraceOpen)
        {
            if let Token::Identifier(name) = &declaration[index] {
                target = Some(name.as_str());
            }
            index += 1;
        }
        let Some(open) = declaration.get(index) else {
            break;
        };
        let close = match open {
            Token::ParenOpen => Token::ParenClose,
            Token::BraceOpen => Token::BraceClose,
            _ => break,
        };
        let open = open.clone();
        let mut depth = 0usize;
        while let Some(token) = declaration.get(index) {
            if token == &open {
                depth += 1;
            } else if token == &close {
                depth -= 1;
                if depth == 0 {
                    index += 1;
                    break;
                }
            }
            index += 1;
        }
        if let Some(target) = target.filter(|target| counted.insert((*target).to_owned())) {
            match fields.0.get(target) {
                Some(ParameterMemberKind::Scalar(scalar_type)) => {
                    facts.scalar_members += 1;
                    scalar_types.insert(scalar_type.as_str());
                }
                Some(ParameterMemberKind::String) => facts.string_members += 1,
                None => {}
            }
        }
        if declaration.get(index) == Some(&Token::Comma) {
            index += 1;
        } else {
            break;
        }
    }
    facts.heterogeneous_scalar_types = scalar_types.len() > 1;
    facts
}

/// Anonymous-label cost of control flow in a dropped in-class definition.
pub(crate) fn inline_control_flow_labels(tokens: &[Token]) -> usize {
    let mut bump = 0;
    let mut condition_pending = false;
    let mut condition_depth = 0i32;
    for token in tokens {
        match token {
            Token::ParenOpen if condition_pending || condition_depth > 0 => {
                condition_depth += 1;
                condition_pending = false;
            }
            Token::ParenClose if condition_depth > 0 => condition_depth -= 1,
            Token::KeywordIf => {
                bump += 2;
                condition_pending = true;
            }
            Token::KeywordWhile => {
                bump += 4;
                condition_pending = true;
            }
            Token::KeywordFor => {
                bump += 5;
                condition_pending = true;
            }
            Token::Identifier(word)
                if matches!(word.as_str(), "else" | "switch" | "case" | "default") =>
            {
                bump += 1;
            }
            Token::Identifier(word) if word == "goto" => bump += 1,
            Token::PipePipe | Token::AmpersandAmpersand if condition_depth > 0 => bump += 1,
            _ => {}
        }
    }
    bump
}

/// Classify a function declaration as `(explicitly_virtual, is_destructor)`.
pub(crate) fn function_declaration_virtuality(
    tokens: &[Token],
    start: usize,
) -> Option<(bool, bool)> {
    let end = tokens[start..]
        .iter()
        .position(|token| matches!(token, Token::Semicolon | Token::BraceOpen))?
        + start;
    let declaration = &tokens[start..end];
    let is_virtual = declaration
        .iter()
        .any(|token| matches!(token, Token::Identifier(word) if word == "virtual"));
    declaration
        .iter()
        .any(|token| token == &Token::ParenOpen)
        .then(|| {
            (
                is_virtual,
                declaration.iter().any(|token| token == &Token::Tilde),
            )
        })
}

/// Count explicit virtual declarations in a nested class tree exactly once.
/// Speculative layout recovery can revisit the same token range.
pub(crate) fn nested_explicit_virtual_declarations(
    tokens: &[Token],
    start: usize,
    counted: &mut HashSet<usize>,
) -> (usize, usize) {
    if !counted.insert(start) {
        return (0, 0);
    }
    let Some(mut index) = tokens[start..]
        .iter()
        .position(|token| token == &Token::BraceOpen)
        .map(|offset| start + offset + 1)
    else {
        return (0, 0);
    };
    let body_start = index;
    let mut result = (0, 0);
    let mut brace_depth = 1i32;
    let mut paren_depth = 0i32;
    while let Some(token) = tokens.get(index) {
        let begins_member = brace_depth == 1
            && paren_depth == 0
            && (index == body_start
                || matches!(
                    tokens.get(index.wrapping_sub(1)),
                    Some(Token::Semicolon | Token::BraceClose)
                )
                || (matches!(tokens.get(index.wrapping_sub(1)), Some(Token::Colon))
                    && matches!(tokens.get(index.wrapping_sub(2)), Some(Token::Identifier(access)) if matches!(access.as_str(), "public" | "private" | "protected"))));
        if begins_member {
            let is_access_label = matches!(token, Token::Identifier(access)
                if matches!(access.as_str(), "public" | "private" | "protected"))
                && tokens.get(index + 1) == Some(&Token::Colon);
            if !is_access_label {
                if let Some((true, is_destructor)) = function_declaration_virtuality(tokens, index)
                {
                    if is_destructor {
                        result.1 += 1;
                    } else {
                        result.0 += 1;
                    }
                }
            }
            if matches!(token, Token::KeywordStruct)
                || matches!(token, Token::Identifier(word) if word == "class")
            {
                let nested = nested_explicit_virtual_declarations(tokens, index, counted);
                result.0 += nested.0;
                result.1 += nested.1;
            }
        }
        match token {
            Token::ParenOpen if brace_depth == 1 => paren_depth += 1,
            Token::ParenClose if brace_depth == 1 && paren_depth > 0 => paren_depth -= 1,
            Token::BraceOpen => brace_depth += 1,
            Token::BraceClose => {
                brace_depth -= 1;
                if brace_depth == 0 {
                    break;
                }
            }
            Token::EndOfFile => break,
            _ => {}
        }
        index += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{constructor_parameter_initializers, parameter_member_fields};
    use mwcc_tokens::Token;

    #[test]
    fn classifies_scalar_and_string_parameter_member_initializers() {
        let tokens = mwcc_source_to_tokens::tokenize(
            r#"
                class Probe {
                    Parm<float> first;
                    Parm<String> title;
                    Probe()
                        : first(this, 1.0f, 0.0f, 0.0f, "p00", "first")
                        , title(this, String("x", 0), String("", 0), String("", 0), "p01", "title")
                    {}
                };
            "#,
        )
        .unwrap();
        let body_start = tokens
            .iter()
            .position(|token| token == &Token::BraceOpen)
            .unwrap()
            + 1;
        let fields = parameter_member_fields(&tokens[body_start..]);
        let constructor = tokens[body_start..]
            .windows(2)
            .position(|pair| {
                matches!(&pair[0], Token::Identifier(name) if name == "Probe")
                    && pair[1] == Token::ParenOpen
            })
            .unwrap()
            + body_start;
        let body = tokens[constructor..]
            .iter()
            .position(|token| token == &Token::BraceOpen)
            .unwrap()
            + constructor;

        let facts = constructor_parameter_initializers(&tokens[constructor..body], &fields);
        assert_eq!(facts.scalar_members, 1);
        assert_eq!(facts.string_members, 1);
        assert!(!facts.heterogeneous_scalar_types);
    }
}
