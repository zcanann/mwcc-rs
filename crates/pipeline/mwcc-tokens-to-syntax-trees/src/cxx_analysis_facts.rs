//! Syntax-only facts consumed by version-specific C++ analysis timelines.
//!
//! These scans must not depend on recoverable object layout: large preprocessed
//! headers can exceed the frontend's current layout subset while still changing
//! MWCC's anonymous-symbol counter.

use std::collections::HashSet;

use mwcc_tokens::Token;

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
