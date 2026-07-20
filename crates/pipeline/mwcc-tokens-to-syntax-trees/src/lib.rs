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
        inline_template_members: std::collections::HashSet::new(),
        inline_cxx_members: std::collections::HashSet::new(),
        cxx_static_methods: HashMap::new(),
        cxx_instance_methods: HashMap::new(),
        cxx_dispatch_tables: HashMap::new(),
        incomplete_cxx_dispatch: std::collections::HashSet::new(),
        template_aliases: HashMap::new(),
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

    #[test]
    fn skips_unused_template_member_specializations_as_inline_materializations() {
        let source = r#"
            template <int N, typename T>
            struct Table { T get(int) const { return 0.0f; } };
            float Table<8, float>::get(int value) const { return 1.0f; }
            int compiled(void) { return 3; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            unit.functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["compiled"]
        );
        assert!(unit.skipped_inline_names.contains("get"));
    }

    #[test]
    fn does_not_skip_an_emitting_template_member_specialization() {
        let source = r#"
            template <int N, typename T>
            struct Table { T get(int) const; };
            float Table<8, float>::get(int value) const { return 1.0f; }
            int compiled(void) { return 3; }
        "#;
        let error = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap_err();
        assert!(error.message.contains("expected ParenOpen, found Less"));
    }

    #[test]
    fn skips_primary_templates_with_default_arguments() {
        let source = r#"
            template <typename T, typename Pointer = T*>
            struct Iterator { typedef Pointer pointer; };
            int compiled(void) { return 3; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "compiled");
    }

    #[test]
    fn resolves_template_aliases_for_inline_specialization_recovery() {
        let source = r#"
            template <typename T>
            struct Table { int get(void) { return 0; } };
            typedef Table<int> IntTable;
            template <> int IntTable::get(void) { return 1; }
            int compiled(void) { return 3; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "compiled");
        assert!(unit.skipped_inline_names.contains("get"));
    }

    #[test]
    fn inherits_inline_from_a_skipped_class_member_declaration() {
        let source = r#"
            namespace N {
            struct C { inline int dropped(int); };
            int C::dropped(int value) { return value; }
            }
            int compiled(void) { return 3; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            unit.functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["compiled"]
        );
        assert!(unit.skipped_inline_names.contains("dropped"));
    }

    #[test]
    fn does_not_skip_an_ordinary_out_of_class_member_definition() {
        let source = r#"
            struct C { int emitted(int); };
            int C::emitted(int value) { return value; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert!(unit.functions[0].name.contains("emitted"));
    }

    #[test]
    fn retains_named_parameter_identity_in_member_definition_symbols() {
        let source = r#"
            struct Creature { int value; };
            struct Action {
                void pointer(const Creature*);
                void reference(const Creature&);
                void value(Creature);
            };
            void Action::pointer(const Creature* creature) { }
            void Action::reference(const Creature& creature) { }
            void Action::value(Creature creature) { }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            unit.functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            [
                "pointer__6ActionFPC8Creature",
                "reference__6ActionFRC8Creature",
                "value__6ActionF8Creature",
            ]
        );
    }

    #[test]
    fn resolves_a_declared_static_cxx_member_call() {
        let source = r#"
            struct System { static void halt(char*, int, char*); };
            void caller(char* message) { System::halt("file", 7, message); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.prototypes[0].0, "halt__6SystemFPciPc");
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name, arguments }
            )] if name == "halt__6SystemFPciPc" && arguments.len() == 3
        ));
    }

    #[test]
    fn resolves_a_variadic_member_call_through_a_global_class_pointer() {
        let source = r#"
            struct Stream { void print(char*, ...); };
            extern Stream* sysCon;
            void caller(char* text) { sysCon->print("%s", text); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.prototypes[0].0, "print__6StreamFPce");
        assert!(unit.variadic_definitions.contains("print__6StreamFPce"));
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name, arguments }
            )] if name == "print__6StreamFPce" && arguments.len() == 3
        ));
    }

    #[test]
    fn resolves_a_virtual_member_to_its_measured_vtable_slot() {
        let source = r#"
            struct Stream {
                virtual int first(void);
                virtual void write(void*, int);
            };
            void caller(Stream* stream, void* bytes, int count) {
                stream->write(bytes, count);
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::VirtualCall {
                    vptr_offset: 0,
                    slot_offset: 12,
                    arguments,
                    ..
                }
            )] if arguments.len() == 2
        ));
    }

    #[test]
    fn inherited_virtual_overrides_reuse_the_base_slot() {
        let source = r#"
            struct Base { virtual int value(int); };
            struct Child : Base {
                int value(int);
                virtual void added(void);
            };
            int caller(Child* child, int input) { return child->value(input); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::VirtualCall {
                vptr_offset: 0,
                slot_offset: 8,
                arguments,
                ..
            }) if arguments.len() == 1
        ));
    }

    #[test]
    fn opaque_reference_and_inline_virtuals_preserve_later_slots() {
        let source = r#"
            struct String;
            struct Stream {
                virtual void first(String&);
                virtual bool ready(void) { return false; }
                virtual void write(void*, int);
            };
            void caller(Stream* stream, void* bytes, int count) {
                stream->write(bytes, count);
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::VirtualCall {
                    slot_offset: 16,
                    ..
                }
            )]
        ));
    }

    #[test]
    fn does_not_classify_explicit_specializations_as_skippable_primary_templates() {
        let source = r#"
            template <typename T> int value(void) { return 1; }
            template <> int value<int>(void) { return 2; }
            int compiled(void) { return 3; }
        "#;
        let error = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("expected a type, found Identifier(\"template\")"));
    }
}
