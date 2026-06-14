//! Pipeline: source text -> tokens (lexing) for the C subset.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_tokens::Token;

pub fn tokenize(source: &str) -> Compilation<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut position = 0;
    let mut tokens = Vec::new();

    while position < bytes.len() {
        let character = bytes[position] as char;

        if character.is_whitespace() {
            position += 1;
            continue;
        }
        // line comment
        if character == '/' && peek(bytes, position + 1) == Some(b'/') {
            while position < bytes.len() && bytes[position] != b'\n' {
                position += 1;
            }
            continue;
        }
        // block comment
        if character == '/' && peek(bytes, position + 1) == Some(b'*') {
            position += 2;
            while position + 1 < bytes.len() && !(bytes[position] == b'*' && bytes[position + 1] == b'/') {
                position += 1;
            }
            position += 2;
            continue;
        }
        // identifier or keyword
        if character.is_ascii_alphabetic() || character == '_' {
            let start = position;
            while position < bytes.len() && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_') {
                position += 1;
            }
            let word = &source[start..position];
            tokens.push(match word {
                "int" => Token::KeywordInt,
                "float" => Token::KeywordFloat,
                "void" => Token::KeywordVoid,
                "return" => Token::KeywordReturn,
                _ => Token::Identifier(word.to_string()),
            });
            continue;
        }
        // hexadecimal literal
        if character == '0' && matches!(peek(bytes, position + 1), Some(b'x') | Some(b'X')) {
            let start = position + 2;
            position += 2;
            while position < bytes.len() && bytes[position].is_ascii_hexdigit() {
                position += 1;
            }
            let value = i64::from_str_radix(&source[start..position], 16)
                .map_err(|_| Diagnostic::error("malformed hexadecimal literal"))?;
            tokens.push(Token::IntegerLiteral(value));
            continue;
        }
        // decimal integer or float literal
        if character.is_ascii_digit() {
            let start = position;
            let mut is_float = false;
            while position < bytes.len() && (bytes[position].is_ascii_digit() || bytes[position] == b'.' || bytes[position] == b'f') {
                if bytes[position] == b'.' {
                    is_float = true;
                }
                if bytes[position] == b'f' {
                    is_float = true;
                    position += 1;
                    break;
                }
                position += 1;
            }
            let text = source[start..position].trim_end_matches('f');
            if is_float {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed float literal"))?;
                tokens.push(Token::FloatLiteral(value));
            } else {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed integer literal"))?;
                tokens.push(Token::IntegerLiteral(value));
            }
            continue;
        }

        let punctuation = match character {
            '(' => Token::ParenOpen,
            ')' => Token::ParenClose,
            '{' => Token::BraceOpen,
            '}' => Token::BraceClose,
            ';' => Token::Semicolon,
            ',' => Token::Comma,
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            other => return Err(Diagnostic::error(format!("unexpected character '{other}'"))),
        };
        tokens.push(punctuation);
        position += 1;
    }

    tokens.push(Token::EndOfFile);
    Ok(tokens)
}

fn peek(bytes: &[u8], index: usize) -> Option<u8> {
    bytes.get(index).copied()
}
