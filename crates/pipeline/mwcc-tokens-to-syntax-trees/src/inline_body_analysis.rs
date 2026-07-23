use mwcc_tokens::Token;

/// Count automatic declarators in a dropped inline body. The 4.3 frontend
/// advances its analysis ordinal once per local even though no body survives
/// code generation. This lexical pass deliberately recognizes declarations,
/// not arbitrary parenthesized expressions.
pub(crate) fn local_declarators(tokens: &[Token], body_open: usize) -> usize {
    if tokens.get(body_open) != Some(&Token::BraceOpen) {
        return 0;
    }

    let mut count = 0usize;
    let mut statement_start = body_open + 1;
    let mut braces = 1usize;
    let mut index = statement_start;
    while let Some(token) = tokens.get(index) {
        match token {
            Token::BraceOpen => {
                braces += 1;
                statement_start = index + 1;
            }
            Token::BraceClose => {
                braces = braces.saturating_sub(1);
                if braces == 0 {
                    break;
                }
                statement_start = index + 1;
            }
            Token::Semicolon => {
                let statement = &tokens[statement_start..index];
                count += declaration_statement(statement);
                statement_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    count
}

/// Return the class name when a dropped inline returns that class by value and
/// declares an automatic of the same type. Wii 4.3 performs a reusable class
/// return/construction analysis for this pattern.
pub(crate) fn same_class_automatic(
    tokens: &[Token],
    declaration_start: usize,
    body_open: usize,
) -> Option<String> {
    let class = tokens
        .get(declaration_start..body_open)?
        .iter()
        .find_map(|token| match token {
            Token::Identifier(word) if !is_declaration_word(word) => Some(word.clone()),
            _ => None,
        })?;
    body_declares_type(tokens, body_open, &class).then_some(class)
}

fn body_declares_type(tokens: &[Token], body_open: usize, class: &str) -> bool {
    if tokens.get(body_open) != Some(&Token::BraceOpen) {
        return false;
    }
    let mut statement_start = body_open + 1;
    let mut depth = 1usize;
    for index in statement_start..tokens.len() {
        match tokens.get(index) {
            Some(Token::BraceOpen) => {
                depth += 1;
                statement_start = index + 1;
            }
            Some(Token::BraceClose) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
                statement_start = index + 1;
            }
            Some(Token::Semicolon) => {
                let statement = trim(&tokens[statement_start..index]);
                let first_type = statement.iter().find_map(|token| match token {
                    Token::Identifier(word) if !is_declaration_word(word) => Some(word.as_str()),
                    _ => None,
                });
                if first_type == Some(class)
                    && statement
                        .iter()
                        .filter(|token| matches!(token, Token::Identifier(word) if !is_declaration_word(word)))
                        .count()
                        >= 2
                {
                    return true;
                }
                statement_start = index + 1;
            }
            _ => {}
        }
    }
    false
}

fn is_declaration_word(word: &str) -> bool {
    matches!(
        word,
        "inline"
            | "__inline"
            | "static"
            | "extern"
            | "const"
            | "volatile"
            | "register"
            | "typename"
            | "class"
            | "struct"
    )
}

fn declaration_statement(mut tokens: &[Token]) -> usize {
    tokens = trim(tokens);
    if tokens.is_empty() {
        return 0;
    }
    if tokens.first() == Some(&Token::KeywordFor) {
        let Some(open) = tokens.iter().position(|token| *token == Token::ParenOpen) else {
            return 0;
        };
        tokens = trim(&tokens[open + 1..]);
    }
    if tokens.is_empty() || starts_expression(tokens) {
        return 0;
    }

    let segments = top_level_segments(tokens);
    let Some(first) = segments.first() else {
        return 0;
    };
    if !first_segment_is_declaration(first) {
        return 0;
    }
    1 + segments
        .iter()
        .skip(1)
        .filter(|segment| declarator_name(segment).is_some())
        .count()
}

fn trim(mut tokens: &[Token]) -> &[Token] {
    while matches!(tokens.first(), Some(Token::Semicolon)) {
        tokens = &tokens[1..];
    }
    tokens
}

fn starts_expression(tokens: &[Token]) -> bool {
    matches!(
        tokens.first(),
        Some(
            Token::KeywordReturn
                | Token::KeywordIf
                | Token::KeywordWhile
                | Token::KeywordDo
                | Token::ParenOpen
                | Token::Star
                | Token::Ampersand
                | Token::Asm
        )
    ) || matches!(tokens.first(), Some(Token::Identifier(word)) if matches!(word.as_str(), "else" | "case" | "default" | "goto" | "asm" | "__asm"))
}

fn top_level_segments(tokens: &[Token]) -> Vec<&[Token]> {
    let mut result = Vec::new();
    let mut start = 0usize;
    let mut parens = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut angles = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::ParenOpen => parens += 1,
            Token::ParenClose => parens = parens.saturating_sub(1),
            Token::BracketOpen => brackets += 1,
            Token::BracketClose => brackets = brackets.saturating_sub(1),
            Token::BraceOpen => braces += 1,
            Token::BraceClose => braces = braces.saturating_sub(1),
            Token::Less if parens == 0 && brackets == 0 && braces == 0 => angles += 1,
            Token::Greater if angles > 0 => angles -= 1,
            Token::Comma if parens == 0 && brackets == 0 && braces == 0 && angles == 0 => {
                result.push(&tokens[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    result.push(&tokens[start..]);
    result
}

fn first_segment_is_declaration(tokens: &[Token]) -> bool {
    let prefix = before_initializer(tokens);
    if prefix.iter().any(
        |token| matches!(token, Token::Identifier(word) if matches!(word.as_str(), "extern" | "static" | "typedef")),
    ) {
        return false;
    }
    if prefix
        .iter()
        .any(|token| matches!(token, Token::Dot | Token::Arrow))
    {
        return false;
    }

    let fundamental = prefix.iter().any(|token| {
        matches!(
            token,
            Token::KeywordInt
                | Token::KeywordChar
                | Token::KeywordShort
                | Token::KeywordUnsigned
                | Token::KeywordFloat
                | Token::KeywordVoid
        ) || matches!(token, Token::Identifier(word) if is_fundamental_word(word))
    });
    let identifiers = top_level_identifiers(prefix);
    if fundamental {
        declarator_name(prefix).is_some()
    } else {
        identifiers.len() >= 2 && declarator_name(prefix).is_some()
    }
}

fn declarator_name(tokens: &[Token]) -> Option<usize> {
    let prefix = before_initializer(tokens);
    top_level_identifiers(prefix).into_iter().last()
}

fn before_initializer(tokens: &[Token]) -> &[Token] {
    let mut nested = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::ParenOpen | Token::BracketOpen | Token::BraceOpen | Token::Less => nested += 1,
            Token::ParenClose | Token::BracketClose | Token::BraceClose | Token::Greater => {
                nested = nested.saturating_sub(1)
            }
            Token::Equals if nested == 0 => return &tokens[..index],
            _ => {}
        }
    }
    tokens
}

fn top_level_identifiers(tokens: &[Token]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut nested = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        match token {
            Token::ParenOpen | Token::BracketOpen | Token::BraceOpen | Token::Less => nested += 1,
            Token::ParenClose | Token::BracketClose | Token::BraceClose | Token::Greater => {
                nested = nested.saturating_sub(1)
            }
            Token::Identifier(word) if nested == 0 && !is_specifier(word) => result.push(index),
            _ => {}
        }
    }
    result
}

fn is_fundamental_word(word: &str) -> bool {
    matches!(word, "bool" | "double" | "long" | "signed" | "wchar_t")
}

fn is_specifier(word: &str) -> bool {
    matches!(
        word,
        "const"
            | "volatile"
            | "register"
            | "static"
            | "auto"
            | "extern"
            | "mutable"
            | "typename"
            | "struct"
            | "class"
            | "union"
            | "enum"
            | "signed"
            | "unsigned"
            | "short"
            | "long"
            | "double"
            | "bool"
            | "wchar_t"
    )
}

#[cfg(test)]
mod tests {
    use super::{local_declarators, same_class_automatic};
    use mwcc_tokens::Token;

    fn count(source: &str) -> usize {
        let tokens = mwcc_source_to_tokens::tokenize(source).unwrap();
        let open = tokens
            .iter()
            .position(|token| *token == mwcc_tokens::Token::BraceOpen)
            .unwrap();
        local_declarators(&tokens, open)
    }

    #[test]
    fn counts_fundamental_typedef_pointer_and_comma_declarations() {
        assert_eq!(
            count("void f() { int first = 1, second = first; f32 value; Type* pointer; }"),
            4
        );
    }

    #[test]
    fn rejects_calls_assignments_casts_and_member_writes() {
        assert_eq!(
            count(
                "void f() { Call(value); left = right; ToU32ref() = color; object.field = 1; \
                 result = *(unsigned char*)pointer; }"
            ),
            0
        );
    }

    #[test]
    fn counts_nested_and_for_initializer_declarations() {
        assert_eq!(
            count("void f() { { const Type& value = source; } for (int i = 0; i < 2; ++i) {} }"),
            2
        );
    }

    #[test]
    fn recognizes_same_class_automatic_in_class_returning_inline() {
        let tokens = mwcc_source_to_tokens::tokenize(
            "inline Value Value::operator+(const Value& rhs) { Value temporary; return temporary; }",
        )
        .unwrap();
        let open = tokens
            .iter()
            .position(|token| *token == Token::BraceOpen)
            .unwrap();
        assert_eq!(
            same_class_automatic(&tokens, 0, open).as_deref(),
            Some("Value")
        );
    }
}
