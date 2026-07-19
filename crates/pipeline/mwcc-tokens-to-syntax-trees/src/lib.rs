//! Pipeline: tokens -> syntax trees (parsing).
//!
//! Recursive descent over the v0 grammar (a function with optional locals and
//! `if`-return guards, then a final return; precedence-climbing expressions).
//! `lib.rs` wires the parser modules and exposes the entry point.

use mwcc_core::Compilation;
use mwcc_syntax_trees::TranslationUnit;
use mwcc_tokens::{LocatedToken, SourceLocation, Token};
use std::collections::HashMap;

mod cxx;
mod expressions;
mod items;
mod parser;

use parser::Parser;

/// Parse a token stream into a translation unit (file-scope globals + the
/// function definition).
pub fn parse_translation_unit(
    tokens: Vec<Token>,
    cplusplus: bool,
    char_is_signed: bool,
    plain_inline_localstatic_base: u8,
    skipped_static_inline_label_base: u8,
) -> Compilation<TranslationUnit> {
    let tokens = tokens
        .into_iter()
        .enumerate()
        .map(|(index, token)| LocatedToken {
            token,
            location: SourceLocation {
                byte_offset: index as u32,
                line: 0,
                column: 0,
            },
        })
        .collect();
    parse_located_translation_unit(
        tokens,
        cplusplus,
        char_is_signed,
        plain_inline_localstatic_base,
        skipped_static_inline_label_base,
    )
}

/// Parse tokens while retaining their physical source positions for DWARF and
/// diagnostics. The token-only entry point remains for synthetic/unit inputs.
pub fn parse_located_translation_unit(
    tokens: Vec<LocatedToken>,
    cplusplus: bool,
    char_is_signed: bool,
    plain_inline_localstatic_base: u8,
    skipped_static_inline_label_base: u8,
) -> Compilation<TranslationUnit> {
    // "East" pointee qualifiers (`u8 const* i`, `int volatile* p`) are
    // codegen-transparent — the qualifier binds the POINTEE, which access
    // codegen doesn't distinguish. Normalize them away when they directly
    // precede the `*` so every parse_type path sees the canonical `u8*`.
    // (`int const g = 5;` KEEPS its const: it routes the global to the
    // read-only section.)
    let mut tokens = cxx::normalize_linkage_specifications(tokens);
    tokens = cxx::normalize_constructor_declarators(tokens);
    let mut index = 0;
    while index + 1 < tokens.len() {
        let is_east_pointee_qualifier = matches!(&tokens[index].token, Token::Identifier(word) if word == "const" || word == "volatile")
            && tokens[index + 1].token == Token::Star;
        if is_east_pointee_qualifier {
            tokens.remove(index);
        } else {
            index += 1;
        }
    }
    let (tokens, locations): (Vec<_>, Vec<_>) = tokens
        .into_iter()
        .map(|located| (located.token, located.location))
        .unzip();
    let mut parser = Parser {
        tokens,
        locations,
        position: 0,
        char_is_signed,
        plain_inline_localstatic_base,
        skipped_static_inline_label_base,
        last_member_array_bytes: None,
        global_structs: std::collections::HashMap::new(),
        block_renames: Vec::new(),
        rename_counter: 0,
        defer_codegen: false,
        deferred_function_names: Vec::new(),
        skipped_inline_functions: 0,
        static_local_prebumps: std::collections::HashMap::new(),
        counted_enum_positions: std::collections::HashSet::new(),
        implicitly_materialized: Vec::new(),
        weak_materialized: Vec::new(),
        weak_functions: std::collections::HashSet::new(),
        static_functions: std::collections::HashSet::new(),
        section_functions: std::collections::HashMap::new(),
        section_prototype_order: Vec::new(),
        skipped_inline_names: std::collections::HashSet::new(),
        inline_bodies: std::collections::HashMap::new(),
        default_cplusplus: cplusplus,
        cplusplus,
        cplusplus_stack: Vec::new(),
        namespace_stack: Vec::new(),
        current_member_scope: None,
        force_active: false,
        structs: HashMap::new(),
        cxx_classes: HashMap::new(),
        struct_templates: HashMap::new(),
        variable_structs: HashMap::new(),
        function_return_structs: HashMap::new(),
        fixed_address_globals: HashMap::new(),
        fixed_address_arrays: HashMap::new(),
        variable_types: HashMap::new(),
        variable_array_bytes: HashMap::new(),
        global_sizes: HashMap::new(),
        last_struct_tag: None,
        asm_parameters: Vec::new(),
        expression_struct_tag: None,
        typedefs: HashMap::new(),
        last_type_was_const: false,
        last_pointer_const: false,
        last_type_was_volatile: false,
        inline_asm_symbols: Vec::new(),
        plain_inline_asm_helpers: Vec::new(),
        struct_typedefs: HashMap::new(),
        struct_pointer_typedefs: HashMap::new(),
        array_typedefs: HashMap::new(),
        row_pointer_typedefs: HashMap::new(),
        last_array_typedef: None,
        decayed_row_pointers: HashMap::new(),
        enum_constants: HashMap::new(),
        function_sources: Vec::new(),
        variadic_definitions: std::collections::HashSet::new(),
        unfolded_float_element: None,
        initializer_pending: Vec::new(),
        pending_sinit: Vec::new(),
    };
    parser.translation_unit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_function_source_boundaries() {
        let raw = [
            (Token::KeywordInt, 1),
            (Token::Identifier("f".into()), 1),
            (Token::ParenOpen, 1),
            (Token::KeywordVoid, 1),
            (Token::ParenClose, 1),
            (Token::BraceOpen, 2),
            (Token::KeywordReturn, 3),
            (Token::IntegerLiteral(3), 3),
            (Token::Semicolon, 3),
            (Token::BraceClose, 4),
            (Token::EndOfFile, 5),
        ];
        let tokens = raw
            .into_iter()
            .enumerate()
            .map(|(index, (token, line))| LocatedToken {
                token,
                location: SourceLocation {
                    byte_offset: index as u32,
                    line,
                    column: 1,
                },
            })
            .collect();

        let unit = parse_located_translation_unit(tokens, false, true, 1, 3).unwrap();
        assert_eq!(
            unit.function_sources,
            [Some(mwcc_syntax_trees::FunctionSource {
                body_start_line: 2,
                terminal_return_line: Some(3),
                body_end_line: 4,
            })]
        );
    }
}
