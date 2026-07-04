//! Pipeline: tokens -> syntax trees (parsing).
//!
//! Recursive descent over the v0 grammar (a function with optional locals and
//! `if`-return guards, then a final return; precedence-climbing expressions).
//! `lib.rs` wires the parser modules and exposes the entry point.

use std::collections::HashMap;
use mwcc_core::Compilation;
use mwcc_syntax_trees::TranslationUnit;
use mwcc_tokens::Token;

mod expressions;
mod items;
mod parser;

use parser::Parser;

/// Parse a token stream into a translation unit (file-scope globals + the
/// function definition).
pub fn parse_translation_unit(tokens: Vec<Token>) -> Compilation<TranslationUnit> {
    // "East" pointee qualifiers (`u8 const* i`, `int volatile* p`) are
    // codegen-transparent — the qualifier binds the POINTEE, which access
    // codegen doesn't distinguish. Normalize them away when they directly
    // precede the `*` so every parse_type path sees the canonical `u8*`.
    // (`int const g = 5;` KEEPS its const: it routes the global to the
    // read-only section.)
    let mut tokens = tokens;
    let mut index = 0;
    while index + 1 < tokens.len() {
        let is_east_pointee_qualifier = matches!(&tokens[index], Token::Identifier(word) if word == "const" || word == "volatile")
            && tokens[index + 1] == Token::Star;
        if is_east_pointee_qualifier {
            tokens.remove(index);
        } else {
            index += 1;
        }
    }
    let mut parser =
        Parser { tokens, position: 0, last_member_array_bytes: None, global_structs: std::collections::HashMap::new(), block_renames: Vec::new(), rename_counter: 0, defer_codegen: false, deferred_function_names: Vec::new(), skipped_inline_functions: 0, static_local_prebumps: std::collections::HashMap::new(), counted_enum_positions: std::collections::HashSet::new(), implicitly_materialized: Vec::new(), weak_functions: std::collections::HashSet::new(), skipped_inline_names: std::collections::HashSet::new(), inline_bodies: std::collections::HashMap::new(), cplusplus: false, cplusplus_stack: Vec::new(), structs: HashMap::new(), variable_structs: HashMap::new(), variable_types: HashMap::new(), variable_array_bytes: HashMap::new(), global_sizes: HashMap::new(), last_struct_tag: None, expression_struct_tag: None, typedefs: HashMap::new(), last_type_was_const: false, last_type_was_volatile: false, inline_asm_symbols: Vec::new(), struct_typedefs: HashMap::new(), struct_pointer_typedefs: HashMap::new(), array_typedefs: HashMap::new(), enum_constants: HashMap::new() };
    parser.translation_unit()
}
