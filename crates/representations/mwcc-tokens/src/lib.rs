//! The token representation: the output of lexing, the input to parsing.

/// A single lexical token of the supported C subset.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // keywords
    KeywordInt,
    KeywordChar,
    KeywordShort,
    KeywordUnsigned,
    KeywordFloat,
    KeywordVoid,
    KeywordReturn,
    KeywordIf,
    KeywordStruct,
    // identifiers and literals
    Identifier(String),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    /// A string literal's decoded bytes (without the surrounding quotes). Codegen
    /// for strings is not in the subset yet — the token lets the lexer get past
    /// `"…"` so the rest of a translation unit still parses.
    StringLiteral(Vec<u8>),
    // punctuation
    ParenOpen,
    ParenClose,
    BraceOpen,
    BraceClose,
    BracketOpen,
    BracketClose,
    Semicolon,
    Comma,
    Equals,
    Question,
    Colon,
    Plus,
    Minus,
    Arrow,
    Dot,
    Star,
    Slash,
    Percent,
    Ampersand,
    Pipe,
    AmpersandAmpersand,
    PipePipe,
    Caret,
    Tilde,
    Bang,
    ShiftLeft,
    ShiftRight,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    EqualEqual,
    BangEqual,
    EndOfFile,
}

impl std::fmt::Display for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}
