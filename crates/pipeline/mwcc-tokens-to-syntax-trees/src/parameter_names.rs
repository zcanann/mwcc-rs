use mwcc_tokens::Token;
use std::collections::HashSet;

/// Collect parameter-name provenance from declaration scopes. Function bodies
/// are skipped wholesale: local declarations are handled by the semantic
/// parser, while expression parentheses must never masquerade as declarators.
pub(crate) fn translation_unit_positions(tokens: &[Token]) -> HashSet<usize> {
    let mut names = HashSet::new();
    let mut index = 0usize;
    while index < tokens.len() {
        if tokens.get(index) == Some(&Token::ParenOpen)
            && could_be_parameter_list(tokens, index)
        {
            if let Some((_, positions)) = positions(tokens, index) {
                names.extend(positions);
            }
        }
        if tokens.get(index) == Some(&Token::BraceOpen) && follows_parameter_list(tokens, index) {
            if let Some(close) = matching_brace(tokens, index) {
                collect_block_prototype_positions(tokens, index + 1, close, &mut names);
                index = close + 1;
                continue;
            }
        }
        index += 1;
    }
    names
}

/// A declaration parameter list follows a function name (or an overloaded
/// operator token). This excludes `sizeof(...)`, casts, calls, and array-bound
/// expressions without requiring the semantic parser to understand the type.
pub(crate) fn could_be_parameter_list(tokens: &[Token], open: usize) -> bool {
    let Some(previous) = open.checked_sub(1).and_then(|index| tokens.get(index)) else {
        return false;
    };
    match previous {
        Token::Identifier(word) => !matches!(
            word.as_str(),
            "sizeof"
                | "alignof"
                | "__alignof__"
                | "decltype"
                | "__decltype__"
                | "typeid"
        ),
        Token::Equals
        | Token::Plus
        | Token::Minus
        | Token::Star
        | Token::Slash
        | Token::Percent
        | Token::Ampersand
        | Token::Pipe
        | Token::Caret
        | Token::Tilde
        | Token::Bang
        | Token::Less
        | Token::Greater
        | Token::LessEqual
        | Token::GreaterEqual
        | Token::EqualEqual
        | Token::BangEqual
        | Token::ShiftLeft
        | Token::ShiftRight
        | Token::BracketClose => declaration_contains_operator(tokens, open),
        _ => false,
    }
}

fn declaration_contains_operator(tokens: &[Token], open: usize) -> bool {
    let start = tokens[..open]
        .iter()
        .rposition(|token| matches!(token, Token::Semicolon | Token::BraceOpen | Token::BraceClose))
        .map_or(0, |position| position + 1);
    tokens[start..open]
        .iter()
        .any(|token| matches!(token, Token::Identifier(word) if word == "operator"))
}

fn collect_block_prototype_positions(
    tokens: &[Token],
    start: usize,
    end: usize,
    names: &mut HashSet<usize>,
) {
    for candidate in start..end {
        if tokens.get(candidate) != Some(&Token::ParenOpen)
            || !parameter_group_begins_with_type(tokens, candidate + 1)
        {
            continue;
        }
        let Some((close, positions)) = positions(tokens, candidate) else {
            continue;
        };
        if close < end && tokens.get(close + 1) == Some(&Token::Semicolon) {
            names.extend(positions);
        }
    }
}

fn parameter_group_begins_with_type(tokens: &[Token], start: usize) -> bool {
    matches!(
        tokens.get(start),
        Some(
            Token::KeywordInt
                | Token::KeywordChar
                | Token::KeywordShort
                | Token::KeywordUnsigned
                | Token::KeywordFloat
                | Token::KeywordVoid
        )
    ) || matches!(tokens.get(start), Some(Token::Identifier(word)) if matches!(word.as_str(), "const" | "volatile" | "signed" | "long" | "double" | "bool" | "wchar_t" | "struct" | "class" | "enum" | "union"))
}

/// Recover source-written parameter-name token positions without requiring the
/// semantic type parser to understand every SDK type. This is a declaration
/// provenance pass only; it never supplies callable types or admits codegen.
pub(crate) fn positions(tokens: &[Token], open: usize) -> Option<(usize, Vec<usize>)> {
    if tokens.get(open) != Some(&Token::ParenOpen) {
        return None;
    }
    let close = matching_paren(tokens, open)?;
    let mut names = Vec::new();
    collect_group(tokens, open + 1, close, &mut names);
    names.sort_unstable();
    names.dedup();
    Some((close, names))
}

fn collect_group(tokens: &[Token], start: usize, end: usize, names: &mut Vec<usize>) {
    if start == end || tokens.get(start..end) == Some(&[Token::KeywordVoid]) {
        return;
    }
    let mut segment_start = start;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    for index in start..=end {
        let boundary = index == end
            || (tokens.get(index) == Some(&Token::Comma)
                && parens == 0
                && brackets == 0
                && braces == 0
                && angles == 0);
        if boundary {
            collect_segment(tokens, segment_start, index, names);
            segment_start = index + 1;
            continue;
        }
        match tokens.get(index) {
            Some(Token::ParenOpen) => parens += 1,
            Some(Token::ParenClose) => parens = parens.saturating_sub(1),
            Some(Token::BracketOpen) => brackets += 1,
            Some(Token::BracketClose) => brackets = brackets.saturating_sub(1),
            Some(Token::BraceOpen) => braces += 1,
            Some(Token::BraceClose) => braces = braces.saturating_sub(1),
            Some(Token::Less) if parens == 0 && brackets == 0 && braces == 0 => angles += 1,
            Some(Token::Greater) if angles > 0 => angles -= 1,
            _ => {}
        }
    }
}

fn collect_segment(tokens: &[Token], start: usize, end: usize, names: &mut Vec<usize>) {
    let end = top_level_default(tokens, start, end);
    if start >= end || is_variadic(&tokens[start..end]) {
        return;
    }

    // A pointer-to-function parameter keeps its name inside `(*name)` (or
    // `(Class::*name)`), outside the top-level declarator stream.
    let mut index = start;
    while index < end {
        if tokens.get(index) == Some(&Token::ParenOpen) {
            if let Some(close) = matching_paren_bounded(tokens, index, end) {
                let group = &tokens[index + 1..close];
                let is_pointer_declarator = group.first() == Some(&Token::Star)
                    || group.windows(3).any(|window| {
                        window[0] == Token::Colon
                            && window[1] == Token::Colon
                            && window[2] == Token::Star
                    });
                if is_pointer_declarator {
                    if let Some(position) = (index + 1..close).rev().find(|position| {
                        matches!(tokens.get(*position), Some(Token::Identifier(word)) if !is_specifier(word))
                    }) {
                        names.push(position);
                    }
                } else {
                    collect_group(tokens, index + 1, close, names);
                }
                index = close + 1;
                continue;
            }
        }
        index += 1;
    }

    let mut depth = 0usize;
    let mut angles = 0usize;
    let mut fundamental = false;
    let mut identifiers = Vec::new();
    for index in start..end {
        match tokens.get(index) {
            Some(Token::ParenOpen | Token::BracketOpen | Token::BraceOpen) => depth += 1,
            Some(Token::ParenClose | Token::BracketClose | Token::BraceClose) => {
                depth = depth.saturating_sub(1)
            }
            Some(Token::Less) if depth == 0 => angles += 1,
            Some(Token::Greater) if depth == 0 && angles > 0 => angles -= 1,
            Some(
                Token::KeywordInt
                | Token::KeywordChar
                | Token::KeywordShort
                | Token::KeywordUnsigned
                | Token::KeywordFloat
                | Token::KeywordVoid,
            ) if depth == 0 && angles == 0 => fundamental = true,
            Some(Token::Identifier(word)) if depth == 0 && angles == 0 => {
                if is_fundamental_word(word) {
                    fundamental = true;
                } else if !is_specifier(word) {
                    identifiers.push(index);
                }
            }
            _ => {}
        }
    }

    let candidate = if fundamental {
        identifiers.last().copied()
    } else if identifiers.len() >= 2 {
        identifiers.last().copied()
    } else {
        None
    };
    if let Some(candidate) = candidate {
        let qualified_type_component = candidate >= start + 2
            && tokens.get(candidate - 1) == Some(&Token::Colon)
            && tokens.get(candidate - 2) == Some(&Token::Colon);
        if !qualified_type_component {
            names.push(candidate);
        }
    }
}

fn top_level_default(tokens: &[Token], start: usize, end: usize) -> usize {
    let mut nested = 0usize;
    for index in start..end {
        match tokens.get(index) {
            Some(Token::ParenOpen | Token::BracketOpen | Token::BraceOpen | Token::Less) => {
                nested += 1
            }
            Some(Token::ParenClose | Token::BracketClose | Token::BraceClose | Token::Greater) => {
                nested = nested.saturating_sub(1)
            }
            Some(Token::Equals) if nested == 0 => return index,
            _ => {}
        }
    }
    end
}

fn matching_paren(tokens: &[Token], open: usize) -> Option<usize> {
    matching_paren_bounded(tokens, open, tokens.len())
}

fn matching_paren_bounded(tokens: &[Token], open: usize, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().take(end).skip(open) {
        match token {
            Token::ParenOpen => depth += 1,
            Token::ParenClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn follows_parameter_list(tokens: &[Token], brace: usize) -> bool {
    let Some(mut previous) = brace.checked_sub(1) else {
        return false;
    };
    while matches!(tokens.get(previous), Some(Token::Identifier(word)) if matches!(word.as_str(), "const" | "volatile" | "override" | "final"))
    {
        let Some(prior) = previous.checked_sub(1) else {
            return false;
        };
        previous = prior;
    }
    tokens.get(previous) == Some(&Token::ParenClose)
}

fn matching_brace(tokens: &[Token], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(open) {
        match token {
            Token::BraceOpen => depth += 1,
            Token::BraceClose => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            Token::EndOfFile => return None,
            _ => {}
        }
    }
    None
}

fn is_variadic(tokens: &[Token]) -> bool {
    tokens.len() == 3 && tokens.iter().all(|token| *token == Token::Dot)
}

fn is_fundamental_word(word: &str) -> bool {
    matches!(word, "signed" | "long" | "double" | "bool" | "wchar_t")
}

fn is_specifier(word: &str) -> bool {
    matches!(
        word,
        "const"
            | "volatile"
            | "register"
            | "auto"
            | "typename"
            | "class"
            | "struct"
            | "enum"
            | "mutable"
            | "restrict"
            | "__restrict"
            | "__restrict__"
    ) || is_fundamental_word(word)
}

#[cfg(test)]
mod tests {
    use super::{positions, translation_unit_positions};
    use mwcc_tokens::Token;
    use std::collections::HashSet;

    #[test]
    fn recovers_ordinary_qualified_template_and_unnamed_parameters() {
        let tokens = vec![
            Token::ParenOpen,
            Token::KeywordInt,
            Token::Identifier("named".into()),
            Token::Comma,
            Token::Identifier("ns".into()),
            Token::Colon,
            Token::Colon,
            Token::Identifier("Type".into()),
            Token::Ampersand,
            Token::Identifier("value".into()),
            Token::Comma,
            Token::Identifier("Vector".into()),
            Token::Less,
            Token::Identifier("float".into()),
            Token::Greater,
            Token::Comma,
            Token::KeywordFloat,
            Token::ParenClose,
        ];
        assert_eq!(positions(&tokens, 0).unwrap().1, vec![2, 9]);
    }

    #[test]
    fn recovers_callback_names_without_charging_the_type_alias() {
        let tokens = vec![
            Token::ParenOpen,
            Token::KeywordVoid,
            Token::ParenOpen,
            Token::Star,
            Token::Identifier("callback".into()),
            Token::ParenClose,
            Token::ParenOpen,
            Token::KeywordInt,
            Token::Identifier("nested".into()),
            Token::ParenClose,
            Token::ParenClose,
        ];
        assert_eq!(positions(&tokens, 0).unwrap().1, vec![4, 8]);
    }

    #[test]
    fn callback_signature_does_not_treat_unnamed_alias_as_a_declarator() {
        let tokens = vec![
            Token::ParenOpen,
            Token::KeywordVoid,
            Token::ParenOpen,
            Token::Star,
            Token::Identifier("visitor".into()),
            Token::ParenClose,
            Token::ParenOpen,
            Token::KeywordVoid,
            Token::Star,
            Token::Comma,
            Token::Identifier("u32".into()),
            Token::ParenClose,
            Token::ParenClose,
        ];
        assert_eq!(positions(&tokens, 0).unwrap().1, vec![4]);
    }

    #[test]
    fn declaration_walk_skips_executable_function_bodies() {
        let tokens = vec![
            Token::Identifier("class".into()),
            Token::Identifier("C".into()),
            Token::BraceOpen,
            Token::KeywordVoid,
            Token::Identifier("method".into()),
            Token::ParenOpen,
            Token::KeywordInt,
            Token::Identifier("member".into()),
            Token::ParenClose,
            Token::BraceOpen,
            Token::Identifier("expression".into()),
            Token::ParenOpen,
            Token::Identifier("left".into()),
            Token::Ampersand,
            Token::Identifier("not_a_parameter".into()),
            Token::ParenClose,
            Token::Semicolon,
            Token::BraceClose,
            Token::KeywordVoid,
            Token::Identifier("declared".into()),
            Token::ParenOpen,
            Token::KeywordFloat,
            Token::Identifier("other".into()),
            Token::ParenClose,
            Token::Semicolon,
            Token::BraceClose,
            Token::Semicolon,
            Token::EndOfFile,
        ];

        assert_eq!(translation_unit_positions(&tokens), HashSet::from([7, 22]));
    }

    #[test]
    fn declaration_walk_rejects_sizeof_array_bounds() {
        let tokens = mwcc_source_to_tokens::tokenize(
            "struct S { int bytes[sizeof(S)]; void method(int value); };",
        )
        .unwrap();
        let value = tokens
            .iter()
            .position(|token| *token == Token::Identifier("value".into()))
            .unwrap();
        assert_eq!(translation_unit_positions(&tokens), HashSet::from([value]));
    }
}
