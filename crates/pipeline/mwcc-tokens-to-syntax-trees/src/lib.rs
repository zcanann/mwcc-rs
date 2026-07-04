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
    let mut parser =
        Parser { tokens, position: 0, last_member_array_bytes: None, global_structs: std::collections::HashMap::new(), block_renames: Vec::new(), rename_counter: 0, defer_codegen: false, deferred_function_names: Vec::new(), skipped_inline_functions: 0, static_local_prebumps: std::collections::HashMap::new(), counted_enum_positions: std::collections::HashSet::new(), weak_functions: std::collections::HashSet::new(), skipped_inline_names: std::collections::HashSet::new(), inline_bodies: std::collections::HashMap::new(), cplusplus: false, cplusplus_stack: Vec::new(), structs: HashMap::new(), variable_structs: HashMap::new(), variable_types: HashMap::new(), variable_array_bytes: HashMap::new(), global_sizes: HashMap::new(), last_struct_tag: None, expression_struct_tag: None, typedefs: HashMap::new(), last_type_was_const: false, last_type_was_volatile: false, inline_asm_symbols: Vec::new(), struct_typedefs: HashMap::new(), struct_pointer_typedefs: HashMap::new(), array_typedefs: HashMap::new(), enum_constants: HashMap::new() };
    parser.translation_unit()
}
