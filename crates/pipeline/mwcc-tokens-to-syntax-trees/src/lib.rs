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
        cxx_inline_ordinal_facts: mwcc_syntax_trees::CxxInlineOrdinalFacts::default(),
        named_prototype_parameters: 0,
        static_local_prebumps: std::collections::HashMap::new(),
        counted_enum_positions: std::collections::HashSet::new(),
        implicitly_materialized: Vec::new(),
        materialized_inline_candidates: Vec::new(),
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
        cxx_namespaces: std::collections::HashSet::new(),
        current_member_scope: None,
        force_active: false,
        peephole_disabled: false,
        structs: HashMap::new(),
        cxx_classes: HashMap::new(),
        struct_templates: HashMap::new(),
        inline_template_members: std::collections::HashSet::new(),
        inline_cxx_members: std::collections::HashSet::new(),
        cxx_static_methods: HashMap::new(),
        cxx_free_functions: HashMap::new(),
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
        global_types: HashMap::new(),
        last_struct_tag: None,
        last_enum_tag: None,
        last_type_was_wchar: false,
        last_type_was_aggregate_reference: false,
        asm_parameters: Vec::new(),
        expression_struct_tag: None,
        typedefs: HashMap::new(),
        function_pointer_typedefs: std::collections::HashSet::new(),
        last_type_was_const: false,
        last_pointer_const: false,
        last_cxx_pointer_depth: 0,
        last_cxx_pointer_base: None,
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
        enum_types: std::collections::HashSet::new(),
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
    fn folds_float_arithmetic_inside_function_expressions() {
        let source = r#"
            int roof(float y) {
                if (y < (-4.0f / 5.0f)) return 1;
                return 0;
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();

        assert!(matches!(
            &unit.functions[0].guards[0].condition,
            mwcc_syntax_trees::Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Less,
                right,
                ..
            } if matches!(right.as_ref(), mwcc_syntax_trees::Expression::FloatLiteral(value)
                if (*value as f32).to_bits() == (-0.8f32).to_bits())
        ));
    }

    #[test]
    fn folds_floating_constant_expressions_cast_to_static_integer_elements() {
        let source = r#"
            void f(void) {
                static short angles[4] = {
                    (short)(0.0f * (65536.0f / 360.0f)),
                    (short)(90.0f * (65536.0f / 360.0f)),
                    (short)(-180.0f * (65536.0f / 360.0f)),
                    (short)(-90.0f * (65536.0f / 360.0f)),
                };
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let angles = unit.functions[0]
            .locals
            .iter()
            .find(|local| local.name == "angles")
            .unwrap();
        assert_eq!(
            angles.data_bytes.as_deref(),
            Some(&[0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0xc0, 0x00][..])
        );
    }

    #[test]
    fn retains_a_nested_block_aggregate_initializer_at_its_execution_point() {
        let source = r#"
            typedef float f32;
            typedef struct Vec { f32 x, y, z; } Vec;
            void use(Vec*);
            void f(int active) {
                if (active) {
                    Vec value = { 1.0f, 2.0f, 3.0f };
                    use(&value);
                }
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let mwcc_syntax_trees::Statement::If { then_body, .. } =
            &unit.functions[0].statements[0]
        else {
            panic!("expected the source if-block");
        };
        assert!(matches!(
            &then_body[0],
            mwcc_syntax_trees::Statement::Assign {
                value: mwcc_syntax_trees::Expression::AggregateLiteral(elements),
                ..
            } if elements.len() == 3
        ));
    }

    #[test]
    fn records_peephole_pragma_scope_on_functions() {
        let source = r#"
            #pragma peephole off
            void preserved(void) {}
            #pragma peephole reset
            void optimized(void) {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(unit.functions[0].peephole_disabled);
        assert!(!unit.functions[1].peephole_disabled);
    }

    #[test]
    fn retains_fixed_address_object_origin_after_expression_desugaring() {
        let source = r#"
            typedef union Pipe { unsigned char u8; unsigned u32; } Pipe;
            volatile Pipe PORT : ((unsigned)((void*)((unsigned)0xCC008000)));
            void write(void) { PORT.u8 = 0x61; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.fixed_address_objects.get("PORT"), Some(&0xCC008000));
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Member {
                    base,
                    member_type: mwcc_syntax_trees::Type::UnsignedChar,
                    ..
                },
                ..
            }] if matches!(base.as_ref(), mwcc_syntax_trees::Expression::Cast { .. })
        ));
    }

    #[test]
    fn parses_asm_qualifier_after_return_type() {
        let source = r#"
            static void asm reset(register int code) {
                nofralloc
                blr
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions[0].name, "reset");
        assert!(unit.functions[0].is_static);
        assert!(unit.functions[0].asm_body.is_some());
    }

    #[test]
    fn resolves_nested_asm_struct_displacements() {
        let source = r#"
            typedef struct Words { unsigned int values[4]; } Words;
            typedef struct StateImpl { int prefix; Words registers; } StateImpl;
            typedef StateImpl State;
            asm void save(void) {
                nofralloc
                lwz r3, State.registers.values[2](r2)
                stw r3, (State.registers.values[1] + 2)(r2)
                ori r3, r4, (1 << (31 - 16))
                blr
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let body = unit.functions[0].asm_body.as_ref().unwrap();
        assert!(matches!(
            &body[1],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[1]
                    == mwcc_syntax_trees::AsmOperand::Memory { displacement: 12, base: 2 }
        ));
        assert!(matches!(
            &body[2],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[1]
                    == mwcc_syntax_trees::AsmOperand::Memory { displacement: 10, base: 2 }
        ));
        assert!(matches!(
            &body[3],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[2] == mwcc_syntax_trees::AsmOperand::Immediate(32768)
        ));
    }

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
            ["compiled__Fv"]
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
        assert_eq!(unit.functions[0].name, "compiled__Fv");
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
        assert_eq!(unit.functions[0].name, "compiled__Fv");
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
            ["compiled__Fv"]
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
    fn accepts_an_opaque_class_reference_in_a_member_definition() {
        let source = r#"
            class Input;
            namespace support {
                template <typename T> class Box {
                    int* pointer;
                };
            }
            class Reader {
            public:
                explicit Reader(Input&);
            private:
                int value;
                support::Box<Input> box;
            };
            Reader::Reader(Input& input) : value(1) { }
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
        assert_eq!(unit.functions[0].name, "__ct__6ReaderFR5Input");
        assert!(matches!(
            unit.functions[0].parameters.as_slice(),
            [
                mwcc_syntax_trees::Parameter { name, .. },
                mwcc_syntax_trees::Parameter {
                    parameter_type: mwcc_syntax_trees::Type::StructPointer { element_size: 0 },
                    ..
                }
            ] if name == "this"
        ));
    }

    #[test]
    fn resolves_a_bare_static_data_member_inside_its_class_method() {
        let source = r#"
            namespace Audio {
                class Bank {
                public:
                    static int current;
                    int read();
                };
                int Bank::current = 0;
                int Bank::read() { return current; }
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

        assert_eq!(unit.globals[0].name, "current__Q25Audio4Bank");
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Variable(name))
                if name == "current__Q25Audio4Bank"
        ));
    }

    #[test]
    fn lowers_a_for_init_declaration_through_block_declaration_rules() {
        let source = r#"
            typedef unsigned int u32;
            int count(void) {
                int sum = 0;
                for (u32 i = 0; i < 3; i++) {
                    sum += i;
                }
                return sum;
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();

        assert!(unit.functions[0]
            .statements
            .iter()
            .any(|statement| matches!(statement, mwcc_syntax_trees::Statement::Loop {
                kind: mwcc_syntax_trees::LoopKind::For,
                initializer: Some(mwcc_syntax_trees::Expression::Assign { .. }),
                ..
            })));
    }

    #[test]
    fn mangles_a_multiply_qualified_member_definition() {
        let source = r#"
            class Outer;
            class Inner {
            public:
                int read() const;
            private:
                int value;
            };
            int Outer::Inner::read() const { return value; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();

        assert_eq!(unit.functions[0].name, "read__Q25Outer5InnerCFv");
    }

    #[test]
    fn mangles_free_cpp_functions_and_preserves_c_linkage() {
        let source = r#"
            extern "C" { int c_api(float); }
            int cpp_api(float);
            int cpp_api(float value) { return c_api(value); }
            int caller(float value) { return cpp_api(value); }
            class Id {
                unsigned short value;
            public:
                int used() const;
            };
            int Id::used() const { return value; }
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
            ["cpp_api__Ff", "caller__Ff", "used__2IdCFv"]
        );
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "c_api"
        ));
        assert!(matches!(
            unit.functions[1].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "cpp_api__Ff"
        ));
    }

    #[test]
    fn resolves_namespace_qualified_free_function_calls_and_definitions() {
        let source = r#"
            namespace std { float sinf(float value); }
            float wrapper(float value) { return std::sinf(value); }
            float std::sinf(float value) { return value; }
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
            ["wrapper__Ff", "sinf__3stdFf"]
        );
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "sinf__3stdFf"
        ));
        assert_eq!(unit.functions[1].parameters.len(), 1);
        assert_eq!(unit.functions[1].parameters[0].name, "value");
    }

    #[test]
    fn treats_anonymous_namespace_as_a_transparent_declaration_scope() {
        let source = r#"
            namespace {
                enum Mode { Off, On };
                int choose(int value) { return value; }
            }
            int wrapper(int value) { return choose(value); }
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
            ["choose__Fi", "wrapper__Fi"]
        );
        assert!(matches!(
            unit.functions[1].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "choose__Fi"
        ));
    }

    #[test]
    fn parses_out_of_class_constructor_and_destructor_definitions() {
        let source = r#"
            class Binder {
            public:
                Binder();
                virtual ~Binder();
            };
            Binder::Binder() {}
            Binder::~Binder() {}
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
            ["__ct__6BinderFv", "__dt__6BinderFv"]
        );
        let destructor = &unit.functions[1];
        assert_eq!(destructor.parameters.len(), 2);
        assert_eq!(destructor.parameters[1].name, "__destroy");
        assert_eq!(destructor.parameters[1].parameter_type, mwcc_syntax_trees::Type::Short);
        let vtable = unit
            .globals
            .iter()
            .find(|global| global.name == "__vt__6Binder")
            .expect("the out-of-line virtual destructor owns the class vtable");
        assert_eq!(vtable.data_bytes.as_deref(), Some(&[0; 12][..]));
        assert_eq!(
            vtable.data_relocations,
            vec![(8, "__dt__6BinderFv".to_string(), 0)]
        );
    }

    #[test]
    fn records_cxx_inline_ordinal_facts_without_assigning_version_weights() {
        let source = r#"
            class Id {
                unsigned short value;
            public:
                virtual ~Id() {}
                Id() { clear(); }
                void clear() { value = 0; }
                unsigned short get() const { return value; }
            };
            int probe() { return 0; }
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
            unit.cxx_inline_ordinal_facts,
            mwcc_syntax_trees::CxxInlineOrdinalFacts {
                class_definitions: 1,
                inline_definitions: 4,
                virtual_destructors: 1,
                direct_calls: 1,
            }
        );
    }

    #[test]
    fn flattens_static_multidimensional_local_initializer_row_major() {
        let source = r#"
            int probe(void) {
                static short values[2][2] = {{1, 2}, {3, 4}};
                return 0;
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let local = &unit.functions[0].locals[0];
        assert_eq!(local.array_length, Some(4));
        assert_eq!(local.row_bytes, Some(4));
        assert_eq!(
            local.data_bytes.as_deref(),
            Some(&[0, 1, 0, 2, 0, 3, 0, 4][..])
        );
    }

    #[test]
    fn inserts_a_vptr_at_the_first_virtual_declaration() {
        let source = r#"
            class Id {
                unsigned short value;
            public:
                void set(int);
                virtual ~Id() {}
            };
            void Id::set(int input) { value = input; }
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
            [mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Member { offset: 0, .. },
                ..
            }]
        ));

        let source = r#"
            class Id {
            public:
                virtual ~Id() {}
                unsigned short value;
                void set(int);
            };
            void Id::set(int input) { value = input; }
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
            [mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Member { offset: 4, .. },
                ..
            }]
        ));
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
    fn keeps_function_pointer_data_member_calls_indirect() {
        let source = r#"
            typedef int (*Callback)(int);
            struct Holder { Callback callback; };
            int caller(Holder* holder) { return holder->callback(7); }
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
            Some(mwcc_syntax_trees::Expression::CallThrough {
                target,
                arguments,
            }) if matches!(target.as_ref(), mwcc_syntax_trees::Expression::Member { offset: 0, .. })
                && arguments.len() == 1
        ));
    }

    #[test]
    fn parses_scoped_function_pointer_typedef_calls() {
        let source = r#"
            void invoke(void* code, void* value) {
                typedef void (*Access)(void*, void*);
                ((Access)code)(value, 0);
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::CallThrough { arguments, .. }
            )] if arguments.len() == 2
        ));
    }

    #[test]
    fn folds_cxx_boolean_literals_in_global_initializers() {
        let source = r#"
            bool enabled = true;
            bool disabled = false;
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.globals.len(), 2);
        assert_eq!(unit.globals[0].initializer, Some(vec![1]));
        assert_eq!(unit.globals[1].initializer, Some(vec![0]));
    }

    #[test]
    fn retains_named_cxx_enum_parameter_identity() {
        let source = r#"
            enum Material { Solid, Water };
            struct Actor { void set(Material); };
            void Actor::set(Material material) {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions[0].name, "set__5ActorF8Material");
        assert_eq!(
            unit.functions[0].parameters[1].parameter_type,
            mwcc_syntax_trees::Type::Int
        );
    }

    #[test]
    fn recovers_mixed_layout_from_a_multi_parameter_template() {
        let source = r#"
            typedef unsigned int uint;
            template <typename T, typename Traits = int, typename Alloc = int>
            class Box {
                struct Metadata { uint capacity; };
                const T* data;
                Metadata* metadata;
                uint size;
                uint padding;
                void ignored(int);
            };
            typedef Box<char> CharBox;
            CharBox value;
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
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 16, align: 4 }
        ));
    }

    #[test]
    fn recovers_wchar_specialization_layout_and_abi_names() {
        let source = r#"
            typedef unsigned int uint;
            template <typename T, typename Traits = int, typename Alloc = int>
            class Box {
                struct Metadata { uint capacity; };
                const T* data;
                Metadata* metadata;
                uint size;
                uint padding;
                void ignored(int);
            };
            typedef Box<wchar_t> WideBox;
            WideBox value;
            struct Text { void set(wchar_t); void ptr(wchar_t*); };
            void Text::set(wchar_t) {}
            void Text::ptr(wchar_t*) {}
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
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 16, align: 4 }
        ));
        assert_eq!(unit.functions[0].name, "set__4TextFw");
        assert_eq!(unit.functions[1].name, "ptr__4TextFPw");
    }

    #[test]
    fn retains_array_typedef_storage_in_union_layouts() {
        let source = r#"
            typedef long Mtx_t[4][4];
            typedef union {
                Mtx_t m;
                long long force_alignment;
            } Mtx;
            typedef struct {
                unsigned char prefix[16];
                Mtx first;
                Mtx second;
                unsigned short tail;
            } Demo;
            Demo value;
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 152, align: 8 }
        ));
    }

    #[test]
    fn retains_named_inline_unions_of_any_size() {
        let source = r#"
            typedef struct {
                unsigned char head;
                union {
                    unsigned char raw;
                    signed char signed_raw;
                } flags;
                union {
                    int words[3];
                    double force_alignment;
                } payload;
                unsigned short tail;
            } Packet;
            Packet value;
            unsigned char raw(Packet* packet) { return packet->flags.raw; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 32, align: 8 }
        ));
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 1, .. })
        ));
    }

    #[test]
    fn preserves_embedded_struct_identity_through_address_of() {
        let source = r#"
            typedef struct Inner { int value; } Inner;
            typedef struct Outer { int prefix; Inner inner; } Outer;
            int read(Outer* outer) { return (&outer->inner)->value; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, base, .. })
                if matches!(base.as_ref(), mwcc_syntax_trees::Expression::AddressOf { .. })
        ));
    }

    #[test]
    fn retains_deep_pointer_members_and_trailing_type_alignment() {
        let source = r#"
            typedef struct {
                unsigned char** animation;
                int count;
            } __attribute__((aligned(32))) TextureAnimation;
            typedef struct {
                int value;
            } PostAligned __attribute__((aligned(32)));
            TextureAnimation first;
            PostAligned second;
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 32, align: 32 }
        ));
        assert!(matches!(
            unit.globals[1].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 4, align: 4 }
        ));
    }

    #[test]
    fn retains_aggregate_sizes_and_member_offsets_above_64k() {
        let source = r#"
            typedef struct {
                unsigned char first[20000];
                unsigned char second[20000];
                unsigned char third[20000];
                unsigned char fourth[20000];
                int tail;
            } Huge;
            Huge value;
            int read_tail(Huge* value) { return value->tail; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct {
                size: 80_004,
                align: 4
            }
        ));
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 80_000, .. })
        ));
    }

    #[test]
    fn honors_alignment_attributes_after_member_declarators() {
        let source = r#"
            typedef struct {
                unsigned char head;
                int value __attribute__((aligned(32)));
                unsigned char tail;
                unsigned char data[7] __attribute__((aligned(16)));
                int end;
            } Layout;
            Layout global;
            int read_value(Layout* value) { return value->value; }
            int read_end(Layout* value) { return value->end; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 64, align: 32 }
        ));
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 32, .. })
        ));
        assert!(matches!(
            unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 56, .. })
        ));
    }

    #[test]
    fn lays_out_adjacent_bit_fields_with_different_storage_types() {
        let source = r#"
            typedef struct {
                unsigned short year : 12;
                unsigned short month : 4;
                unsigned char day : 5;
                unsigned char day_pad : 3;
                unsigned char hour : 5;
                unsigned char hour_pad : 3;
                unsigned char quarter : 4;
                unsigned char active : 1;
                unsigned char final_pad : 3;
                unsigned char end;
            } MixedBits;
            unsigned char read_end(MixedBits* value) { return value->end; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 5, .. })
        ));
    }

    #[test]
    fn defers_unlowered_kr_function_definitions_instead_of_dropping_text() {
        let source = r#"
            int add(left, right)
            int left;
            int right;
            {
                return left + right;
            }
        "#;
        let error = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap_err();
        assert!(error.message.contains("expected a type"));
    }

    #[test]
    fn does_not_mistake_function_type_declarations_for_kr_definitions() {
        let source = r#"
            typedef int (save_check_proc)(void);
            int old_style_declaration(left, right);
            int answer(void) { return 42; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "answer__Fv");
    }

    #[test]
    fn resolves_sizeof_through_pointer_and_array_members() {
        let source = r#"
            typedef struct Holder {
                int* values;
                char bytes[8];
            } Holder;
            int sizes(Holder* holder) {
                return sizeof(*holder->values) + sizeof(holder->bytes[0]);
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            &unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::IntegerLiteral(5))
        ));
    }

    #[test]
    fn resolves_sizeof_through_a_global_struct_pointer() {
        let source = r#"
            struct Data {
                int first;
                short second;
            };
            extern struct Data* gx;
            int size(void) { return sizeof(*gx); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            &unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::IntegerLiteral(8))
        ));
    }

    #[test]
    fn retains_struct_layout_across_static_cxx_method_declarations() {
        let source = r#"
            struct Slice {
                char* text;
                unsigned size;
                static Slice create(const char* text, unsigned size);
            };
            unsigned length(Slice* slice) { return slice->size; }
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
            &unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 4, .. })
        ));
    }

    #[test]
    fn initializes_the_first_declared_union_member_deterministically() {
        let source = r#"
            typedef struct {
                unsigned char color[3];
                char pad;
            } Color;
            typedef union {
                Color value;
                long long force_alignment[1];
            } ColorUnion;
            typedef struct {
                ColorUnion entries[1];
            } Material;
            static Material material = { { { {{1, 2, 3}, 0} } } };
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            &unit.globals[0].data_bytes.as_ref().unwrap()[..4],
            &[1, 2, 3, 0]
        );
    }

    #[test]
    fn serializes_nested_block_static_scalar_and_struct_initializers() {
        let source = r#"
            typedef struct Color {
                unsigned char r, g, b, a;
            } Color;
            void initialize(void) {
                int before = 0;
                {
                    static Color light = {90, 90, 45, 255};
                    static unsigned command = 0 << 24;
                    static unsigned marker = 1 << 24;
                }
                before = 1;
            }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let locals = &unit.functions[0].locals;
        let light = locals.iter().find(|local| local.name == "light").unwrap();
        let command = locals
            .iter()
            .find(|local| local.name == "command")
            .unwrap();
        let marker = locals
            .iter()
            .find(|local| local.name == "marker")
            .unwrap();
        assert_eq!(light.data_bytes.as_deref(), Some(&[90, 90, 45, 255][..]));
        assert_eq!(command.data_bytes.as_deref(), Some(&[0, 0, 0, 0][..]));
        assert_eq!(marker.data_bytes.as_deref(), Some(&[1, 0, 0, 0][..]));
        assert!(light.is_static && command.is_static && marker.is_static);
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
    fn parses_explicit_function_specializations_as_concrete_definitions() {
        let source = r#"
            template <typename T> int value(T value);
            template <> int value(int input) { return input + 2; }
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
        assert_eq!(unit.functions[0].name, "value__Fi");
    }

    #[test]
    fn retains_double_pointer_identity_in_cxx_function_mangling() {
        let source = r#"
            char* xStrTok(char* string, const char* control, char** nextoken) {
                return string;
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
        assert_eq!(unit.functions[0].name, "xStrTok__FPcPCcPPc");
    }

    #[test]
    fn resolves_exact_cxx_overloads_from_dereferenced_argument_types() {
        let source = r#"
            int lower(char value) { return value; }
            int lower(int value) { return value; }
            int use(char* text) { return lower(text[0]); }
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
            &unit.functions[2].return_expression,
            Some(mwcc_syntax_trees::Expression::Call { name, .. })
                if name == "lower__Fc"
        ));
    }

    #[test]
    fn leaves_primary_templates_on_the_recovery_path() {
        let source = r#"
            template <typename T> int value(T value) { return value; }
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
        assert_eq!(unit.functions[0].name, "compiled__Fv");
    }

    #[test]
    fn drops_unused_specializations_of_inline_class_template_members() {
        let source = r#"
            namespace J {
                template <typename T, class Allocator = T>
                struct Vector {
                    T* begin;
                    unsigned capacity;
                    void** Insert_raw(T* first, unsigned count) { return 0; }
                };
            }
            typedef J::Vector<void*, void*> VectorAlias;
            template <>
            void** VectorAlias::Insert_raw(void** first, unsigned count) {
                return first;
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
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "compiled__Fv");
    }
}
