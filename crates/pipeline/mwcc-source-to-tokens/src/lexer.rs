//! The lexer: source text -> a flat token stream for the C subset.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_tokens::{LocatedToken, SourceLocation, Token};

pub fn tokenize(source: &str) -> Compilation<Vec<Token>> {
    tokenize_bytes(source.as_bytes())
}

pub fn tokenize_located(source: &str) -> Compilation<Vec<LocatedToken>> {
    tokenize_bytes_located(source.as_bytes())
}

/// Tokenize a source file without imposing a Unicode encoding on it.
///
/// C's grammar is ASCII and Metrowerks' `-multibyte` inputs commonly carry
/// Shift-JIS payloads in comments and string literals. Keeping the input as
/// bytes preserves those literal payloads exactly; identifier and numeric
/// slices are converted only after their ASCII scanners have accepted them.
pub fn tokenize_bytes(bytes: &[u8]) -> Compilation<Vec<Token>> {
    Ok(tokenize_bytes_located(bytes)?
        .into_iter()
        .map(|located| located.token)
        .collect())
}

pub fn tokenize_bytes_located(bytes: &[u8]) -> Compilation<Vec<LocatedToken>> {
    let mut position = 0;
    let mut tokens = Vec::new();
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(
            bytes
                .iter()
                .enumerate()
                .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1)),
        )
        .collect();
    macro_rules! push_token {
        ($token:expr, $offset:expr) => {
            tokens.push(LocatedToken {
                token: $token,
                location: source_location(&line_starts, $offset),
            })
        };
    }
    // Inline-`asm` block tracking. `expect_asm_block` is armed when an `asm`
    // keyword is seen; the NEXT `{` opens an asm block (or a `;` disarms it — an
    // `asm`-qualified prototype has no body). Inside a block (`asm_depth > 0`)
    // newlines become `Token::Newline` so the parser can group instructions by
    // line. asm bodies contain no nested braces, so depth only ever reaches 1.
    let mut expect_asm_block = false;
    let mut asm_depth: u32 = 0;
    // Names of functions declared `asm` (in a prototype or definition). A later
    // definition that omits the `asm` keyword is still lexed as an asm body.
    let mut asm_function_names = std::collections::HashSet::new();
    // The most recent identifier seen while an `asm` declaration signature is open —
    // resolved to the function name at its `(`.
    let mut pending_asm_name: Option<String> = None;

    while position < bytes.len() {
        let character = bytes[position] as char;

        // A pending asm block that hits a `;` before its `{` was a prototype.
        if expect_asm_block && character == ';' {
            expect_asm_block = false;
        }
        // Enter an asm block at the `{` following an `asm` qualifier.
        if expect_asm_block && character == '{' {
            expect_asm_block = false;
            asm_depth = 1;
            push_token!(Token::BraceOpen, position);
            position += 1;
            continue;
        }
        // Record the name of an `asm`-declared function at its `(` — so a later
        // DEFINITION that OMITS the `asm` keyword (BfBB's `asm void f(void);` proto
        // then `void f(void){…}` body) is still recognized as an asm body below.
        if expect_asm_block && character == '(' {
            if let Some(name) = pending_asm_name.take() {
                asm_function_names.insert(name);
            }
        }
        // Inside an asm block: a newline separates instructions; a `}` closes it.
        if asm_depth > 0 {
            if character == '\n' {
                push_token!(Token::Newline, position);
                position += 1;
                continue;
            }
            if character == '}' {
                asm_depth -= 1;
                push_token!(Token::BraceClose, position);
                position += 1;
                continue;
            }
        }

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
            if let Ok(line) = std::str::from_utf8(&bytes[line_start..position]) {
                let directive = line.trim().trim_start_matches('#').trim();
                if let Some(rest) = directive.strip_prefix("pragma ") {
                    let rest = rest.trim();
                    if matches!(rest, "cplusplus on" | "cplusplus off" | "cplusplus reset" | "push" | "pop" | "defer_codegen on" | "defer_codegen off" | "force_active on" | "force_active off" | "force_active reset") {
                        push_token!(Token::Pragma(rest.to_string()), line_start);
                    }
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
            let token_start = position;
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
            push_token!(Token::StringLiteral(content), token_start);
            continue;
        }
        // A wide literal's `L` prefix (`L'\0'`, `L"..."`) is transparent to the
        // VALUE for a char literal (wchar_t is an integer type); a wide STRING
        // changes the data layout (u16 elements) and defers at codegen, so the
        // `L` before `"` simply drops here and the string lexes normally.
        if character == 'L' && matches!(peek(bytes, position + 1), Some(b'\'') | Some(b'"')) {
            position += 1;
            continue;
        }
        // Character literal: one byte (`'c'`, `'\n'`) or a Metrowerks-style
        // multi-character tag (`'fp00'`). mwcc packs up to four source bytes
        // big-endian into the `int` value, matching the target byte order.
        if character == '\'' {
            let token_start = position;
            position += 1;
            let mut value = 0u32;
            let mut count = 0u8;
            loop {
                let byte = match peek(bytes, position) {
                    None | Some(b'\n' | b'\r') => {
                        return Err(Diagnostic::error("unterminated character literal"));
                    }
                    Some(b'\'') if count == 0 => {
                        return Err(Diagnostic::error("empty character literal"));
                    }
                    Some(b'\'') => {
                        position += 1;
                        break;
                    }
                    Some(b'\\') => {
                        position += 1;
                        let escaped = *bytes.get(position).ok_or_else(|| {
                            Diagnostic::error("unterminated character literal")
                        })?;
                        position += 1;
                        match escaped {
                            b'n' => 10,
                            b't' => 9,
                            b'r' => 13,
                            b'0' => 0,
                            b'a' => 7,
                            b'b' => 8,
                            b'f' => 12,
                            b'v' => 11,
                            other => other,
                        }
                    }
                    Some(byte) => {
                        position += 1;
                        byte
                    }
                };
                count += 1;
                if count > 4 {
                    return Err(Diagnostic::error(
                        "character literal contains more than four bytes",
                    ));
                }
                value = (value << 8) | u32::from(byte);
            }
            push_token!(Token::IntegerLiteral(i64::from(value)), token_start);
            continue;
        }
        // identifier or keyword
        if character.is_ascii_alphabetic() || character == '_' {
            let start = position;
            while position < bytes.len() && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_') {
                position += 1;
            }
            let word = std::str::from_utf8(&bytes[start..position])
                .expect("the identifier scanner accepts only ASCII bytes");
            let token = match word {
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
                "asm" => Token::Asm,
                _ => Token::Identifier(word.to_string()),
            };
            // Arm asm-block tracking so the next `{` opens a verbatim asm body.
            if token == Token::Asm {
                expect_asm_block = true;
                pending_asm_name = None;
            } else if let Token::Identifier(name) = &token {
                if expect_asm_block {
                    // Inside an `asm` signature: this is a candidate function name.
                    pending_asm_name = Some(name.clone());
                } else if asm_function_names.contains(name)
                    // A keyword-less DEFINITION of an already-`asm`-declared function
                    // (BfBB): the name follows a single-keyword return type. Splice a
                    // synthetic `asm` token before that type so the parser dispatches
                    // to its asm path exactly as if the keyword were present.
                    && matches!(
                        tokens.last().map(|located| &located.token),
                        Some(Token::KeywordVoid | Token::KeywordInt | Token::KeywordChar | Token::KeywordShort | Token::KeywordUnsigned | Token::KeywordFloat)
                    )
                {
                    let insertion = tokens.len() - 1;
                    let location = tokens[insertion].location;
                    tokens.insert(
                        insertion,
                        LocatedToken {
                            token: Token::Asm,
                            location,
                        },
                    );
                    expect_asm_block = true;
                    pending_asm_name = Some(name.clone());
                }
            }
            push_token!(token, start);
            continue;
        }
        // hexadecimal literal
        if character == '0' && matches!(peek(bytes, position + 1), Some(b'x') | Some(b'X')) {
            let token_start = position;
            let start = position + 2;
            position += 2;
            while position < bytes.len() && bytes[position].is_ascii_hexdigit() {
                position += 1;
            }
            // Parse as u64 and wrap: a full-width literal (0xFFFFFFFFFFFFFFFF)
            // overflows i64 but is a valid C constant (its bits are the value).
            let text = std::str::from_utf8(&bytes[start..position])
                .expect("the hexadecimal scanner accepts only ASCII bytes");
            let value = u64::from_str_radix(text, 16)
                .map_err(|_| Diagnostic::error("malformed hexadecimal literal"))? as i64;
            position = consume_integer_suffix(bytes, position);
            push_token!(Token::IntegerLiteral(value), token_start);
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
            let text = std::str::from_utf8(&bytes[start..position])
                .expect("the numeric scanner accepts only ASCII bytes")
                .trim_end_matches(['f', 'F']);
            position = consume_integer_suffix(bytes, position);
            if is_float {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed float literal"))?;
                push_token!(Token::FloatLiteral(value), start);
            } else {
                let value = text.parse().map_err(|_| Diagnostic::error("malformed integer literal"))?;
                push_token!(Token::IntegerLiteral(value), start);
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
            push_token!(token, position);
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
        push_token!(punctuation, position);
        position += 1;
    }

    push_token!(Token::EndOfFile, bytes.len());
    Ok(tokens)
}

fn source_location(line_starts: &[usize], byte_offset: usize) -> SourceLocation {
    let line_index = line_starts.partition_point(|start| *start <= byte_offset) - 1;
    SourceLocation {
        byte_offset: byte_offset as u32,
        line: line_index as u32 + 1,
        column: (byte_offset - line_starts[line_index]) as u32 + 1,
    }
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

#[cfg(test)]
mod tests {
    use super::{tokenize_bytes, tokenize_bytes_located};
    use mwcc_tokens::Token;

    #[test]
    fn shift_jis_string_payload_is_preserved_byte_for_byte() {
        let tokens = tokenize_bytes(b"char *s = \"\x83\x8a\x83\x93\x83N\";").unwrap();
        assert!(tokens.contains(&Token::StringLiteral(vec![
            0x83, 0x8a, 0x83, 0x93, 0x83, 0x4e,
        ])));
    }

    #[test]
    fn non_utf8_comment_bytes_do_not_affect_c_tokens() {
        let tokens = tokenize_bytes(b"/* \x94g\x96\xe4 */ int f(void) { return 3; }").unwrap();
        assert!(tokens.contains(&Token::Identifier("f".to_string())));
        assert!(tokens.contains(&Token::IntegerLiteral(3)));
    }

    #[test]
    fn multi_character_constants_pack_source_bytes_big_endian() {
        let tokens = tokenize_bytes(b"int tag = 'fp00';").unwrap();
        assert!(tokens.contains(&Token::IntegerLiteral(0x6670_3030)));
    }

    #[test]
    fn located_tokens_retain_physical_lines_and_columns() {
        let tokens = tokenize_bytes_located(b"int f(void) {\n  return 3;\n}\n").unwrap();
        let return_token = tokens
            .iter()
            .find(|located| located.token == Token::KeywordReturn)
            .unwrap();
        assert_eq!(return_token.location.line, 2);
        assert_eq!(return_token.location.column, 3);
        assert_eq!(return_token.location.byte_offset, 16);
    }
}
