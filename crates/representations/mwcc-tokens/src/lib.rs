//! The token representation: the output of lexing, the input to parsing.

/// A single lexical token of the supported C subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // keywords
    KeywordInt,
    KeywordFloat,
    KeywordVoid,
    KeywordReturn,
    // identifiers and literals
    Identifier(String),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    // punctuation
    ParenOpen,
    ParenClose,
    BraceOpen,
    BraceClose,
    Semicolon,
    Comma,
    Equals,
    Plus,
    Minus,
    Star,
    Slash,
    EndOfFile,
}

impl std::fmt::Display for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
