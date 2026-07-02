//! The lexer: source text -> a flat token stream for the C subset.

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
        // A preprocessor directive surviving in the `-E` output (mwcc passes
        // `#pragma` through). It is not a C token; skip the line. (A directive
        // that changes codegen, like `#pragma section`, is handled downstream.)
        if character == '#' {
            let line_start = position;
            while position < bytes.len() && bytes[position] != b'\n' {
                position += 1;
            }
            // `#pragma cplusplus on/off` and the `push`/`pop` scoping around it
            // switch the LANGUAGE for the enclosed declarations (their symbol
            // names mangle) — surface those; every other directive is skipped.
            let line = source[line_start..position].trim();
            let directive = line.trim_start_matches('#').trim();
            if let Some(rest) = directive.strip_prefix("pragma ") {
                let rest = rest.trim();
                if matches!(rest, "cplusplus on" | "cplusplus off" | "push" | "pop") {
                    tokens.push(Token::Pragma(rest.to_string()));
                }
            }
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
        // string literal: `"…"` with escapes, decoded to its bytes. (Codegen for
        // strings isn't in the subset; lexing lets the rest of the unit parse.)
        if character == '"' {
            position += 1;
            let mut content = Vec::new();
            loop {
                match peek(bytes, position) {
                    None => return Err(Diagnostic::error("unterminated string literal")),
                    Some(b'"') => {
                        position += 1;
                        break;
                    }
                    Some(b'\\') => {
                        position += 1;
                        let escaped = *bytes.get(position).ok_or_else(|| Diagnostic::error("unterminated string literal"))?;
                        position += 1;
                        content.push(match escaped {
                            b'n' => 10, b't' => 9, b'r' => 13, b'0' => 0, b'a' => 7,
                            b'b' => 8, b'f' => 12, b'v' => 11, other => other,
                        });
                    }
                    Some(byte) => {
                        content.push(byte);
                        position += 1;
                    }
                }
            }
            tokens.push(Token::StringLiteral(content));
            continue;
        }
        // character literal: `'c'` or an escape like `'\n'`, yielding the
        // character's integer value (an `int`-typed constant in C).
        if character == '\'' {
            position += 1;
            let value = if peek(bytes, position) == Some(b'\\') {
                position += 1;
                let escaped = *bytes.get(position).ok_or_else(|| Diagnostic::error("unterminated character literal"))?;
                position += 1;
                match escaped {
                    b'n' => 10, b't' => 9, b'r' => 13, b'0' => 0, b'a' => 7,
                    b'b' => 8, b'f' => 12, b'v' => 11, other => other as i64,
                }
            } else {
                let byte = *bytes.get(position).ok_or_else(|| Diagnostic::error("unterminated character literal"))?;
                position += 1;
                byte as i64
            };
            if peek(bytes, position) != Some(b'\'') {
                return Err(Diagnostic::error("unterminated or multi-character literal"));
            }
            position += 1;
            tokens.push(Token::IntegerLiteral(value));
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
                "char" => Token::KeywordChar,
                "short" => Token::KeywordShort,
                "unsigned" => Token::KeywordUnsigned,
                "float" => Token::KeywordFloat,
                "void" => Token::KeywordVoid,
                "return" => Token::KeywordReturn,
                "if" => Token::KeywordIf,
                "while" => Token::KeywordWhile,
                "do" => Token::KeywordDo,
                "for" => Token::KeywordFor,
                "struct" => Token::KeywordStruct,
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
            // Parse as u64 and wrap: a full-width literal (0xFFFFFFFFFFFFFFFF)
            // overflows i64 but is a valid C constant (its bits are the value).
            let value = u64::from_str_radix(&source[start..position], 16)
                .map_err(|_| Diagnostic::error("malformed hexadecimal literal"))? as i64;
            position = consume_integer_suffix(bytes, position);
            tokens.push(Token::IntegerLiteral(value));
            continue;
        }
        // decimal integer or float literal (a leading-dot float `.5` counts:
        // C allows the omitted integer part).
        if character.is_ascii_digit() || (character == '.' && peek(bytes, position + 1).is_some_and(|byte| byte.is_ascii_digit())) {
            let start = position;
            let mut is_float = false;
            while position < bytes.len() {
                let byte = bytes[position];
                if byte.is_ascii_digit() || byte == b'.' {
                    if byte == b'.' {
                        is_float = true;
                    }
                    position += 1;
                } else if (byte == b'e' || byte == b'E')
                    && matches!(peek(bytes, position + 1), Some(b'0'..=b'9') | Some(b'+') | Some(b'-'))
                {
                    // Scientific-notation exponent `e[+-]?digits` (`1.0e300`, `2.5e-10`,
                    // `1e10`) — always a float, even without a fractional dot.
                    is_float = true;
                    position += 1; // the `e`/`E`
                    if matches!(peek(bytes, position), Some(b'+') | Some(b'-')) {
                        position += 1;
                    }
                    while position < bytes.len() && bytes[position].is_ascii_digit() {
                        position += 1;
                    }
                } else if byte == b'f' || byte == b'F' {
                    is_float = true;
                    position += 1;
                    break;
                } else {
                    break;
                }
            }
            let text = source[start..position].trim_end_matches(['f', 'F']);
            position = consume_integer_suffix(bytes, position);
            if is_float {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed float literal"))?;
                tokens.push(Token::FloatLiteral(value));
            } else {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed integer literal"))?;
                tokens.push(Token::IntegerLiteral(value));
            }
            continue;
        }

        // two-character operators
        let two = (character, peek(bytes, position + 1));
        let two_char = match two {
            ('<', Some(b'<')) => Some(Token::ShiftLeft),
            ('>', Some(b'>')) => Some(Token::ShiftRight),
            ('<', Some(b'=')) => Some(Token::LessEqual),
            ('>', Some(b'=')) => Some(Token::GreaterEqual),
            ('=', Some(b'=')) => Some(Token::EqualEqual),
            ('!', Some(b'=')) => Some(Token::BangEqual),
            ('&', Some(b'&')) => Some(Token::AmpersandAmpersand),
            ('|', Some(b'|')) => Some(Token::PipePipe),
            ('-', Some(b'>')) => Some(Token::Arrow),
            ('+', Some(b'+')) => Some(Token::PlusPlus),
            ('-', Some(b'-')) => Some(Token::MinusMinus),
            _ => None,
        };
        if let Some(token) = two_char {
            tokens.push(token);
            position += 2;
            continue;
        }

        let punctuation = match character {
            '(' => Token::ParenOpen,
            ')' => Token::ParenClose,
            '{' => Token::BraceOpen,
            '}' => Token::BraceClose,
            '[' => Token::BracketOpen,
            ']' => Token::BracketClose,
            ';' => Token::Semicolon,
            ',' => Token::Comma,
            '=' => Token::Equals,
            '?' => Token::Question,
            ':' => Token::Colon,
            '+' => Token::Plus,
            '-' => Token::Minus,
            '*' => Token::Star,
            '/' => Token::Slash,
            '%' => Token::Percent,
            '&' => Token::Ampersand,
            '|' => Token::Pipe,
            '^' => Token::Caret,
            '~' => Token::Tilde,
            '!' => Token::Bang,
            '<' => Token::Less,
            '>' => Token::Greater,
            // A standalone `.` is member access; `.` inside a number is consumed
            // by the literal lexer above.
            '.' => Token::Dot,
            // `@` only occurs inside a Metrowerks inline-`asm` block (a local label
            // `@2` or a reloc suffix `sym@ha`). Tokenize it so one asm-bodied function
            // does not turn the whole file into a lex-error; the asm function defers.
            '@' => Token::At,
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

/// Advance past an integer literal's type-suffix letters (`u`/`U`/`l`/`L` and
/// combinations like `UL`, `LL`, `ULL`). On this 32-bit target these are hints
/// only — they don't change the literal's value — so they are consumed and dropped
/// (otherwise `0x10U` would leave a stray `U` identifier behind).
fn consume_integer_suffix(bytes: &[u8], mut position: usize) -> usize {
    while matches!(peek(bytes, position), Some(b'u' | b'U' | b'l' | b'L')) {
        position += 1;
    }
    position
}
