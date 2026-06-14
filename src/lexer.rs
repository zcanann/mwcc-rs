//! Minimal C lexer for the v0 subset.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // keywords
    Int,
    Float,
    Void,
    Return,
    // literals / identifiers
    Ident(String),
    IntLit(i64),
    FloatLit(f64),
    // punctuation
    LParen,
    RParen,
    LBrace,
    RBrace,
    Semi,
    Comma,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

pub fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        // line + block comments
        if c == '/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = &src[start..i];
            out.push(match word {
                "int" => Tok::Int,
                "float" => Tok::Float,
                "void" => Tok::Void,
                "return" => Tok::Return,
                _ => Tok::Ident(word.to_string()),
            });
            continue;
        }
        // hex literal
        if c == '0' && i + 1 < b.len() && (b[i + 1] == b'x' || b[i + 1] == b'X') {
            let start = i + 2;
            i += 2;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
            let v = i64::from_str_radix(&src[start..i], 16).map_err(|_| "bad hex".to_string())?;
            out.push(Tok::IntLit(v));
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            let mut is_float = false;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'f') {
                if b[i] == b'.' {
                    is_float = true;
                }
                if b[i] == b'f' {
                    // trailing float suffix; consume and stop
                    is_float = true;
                    i += 1;
                    break;
                }
                i += 1;
            }
            let text = src[start..i].trim_end_matches('f');
            if is_float {
                out.push(Tok::FloatLit(text.parse().map_err(|_| format!("bad float {text}"))?));
            } else {
                out.push(Tok::IntLit(text.parse().map_err(|_| format!("bad int {text}"))?));
            }
            continue;
        }
        let single = match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            ';' => Tok::Semi,
            ',' => Tok::Comma,
            '+' => Tok::Plus,
            '-' => Tok::Minus,
            '*' => Tok::Star,
            '/' => Tok::Slash,
            _ => return Err(format!("unexpected char '{c}' at {i}")),
        };
        out.push(single);
        i += 1;
    }
    out.push(Tok::Eof);
    Ok(out)
}
