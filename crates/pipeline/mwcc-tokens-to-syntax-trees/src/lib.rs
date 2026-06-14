//! Pipeline: tokens -> syntax trees (parsing).
//!
//! Recursive descent over the v0 grammar (a function with optional locals and
//! `if`-return guards, then a final return; precedence-climbing expressions).
//! `lib.rs` wires the parser modules and exposes the entry point.

use mwcc_core::Compilation;
use mwcc_syntax_trees::Function;
use mwcc_tokens::Token;

mod expressions;
mod items;
mod parser;

use parser::Parser;

/// Parse a token stream into a single function definition.
pub fn parse_function(tokens: Vec<Token>) -> Compilation<Function> {
    let mut parser = Parser { tokens, position: 0 };
    parser.function()
}
