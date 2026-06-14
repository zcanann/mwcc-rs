//! Pipeline: source text -> tokens (lexing). `lib.rs` re-exports the lexer.

mod lexer;

pub use lexer::tokenize;
