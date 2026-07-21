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
mod cxx_analysis_facts;
mod cxx_new;
mod cxx_rtti;
mod expressions;
mod items;
mod parser;

use parser::Parser;

pub use cxx_rtti::materialize as materialize_cxx_rtti;

pub(crate) const CXX_POINTEE_CONST_MARKER: &str = "__mwcc_cxx_pointee_const";

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
    parse_located_translation_unit_with_enum_min(
        tokens,
        cplusplus,
        char_is_signed,
        plain_inline_localstatic_base,
        skipped_static_inline_label_base,
        false,
    )
}

/// Parse tokens with an explicit enumeration-storage policy. The compatibility
/// entry points above retain mwcc's `-enum int` default for existing callers.
pub fn parse_located_translation_unit_with_enum_min(
    tokens: Vec<LocatedToken>,
    cplusplus: bool,
    char_is_signed: bool,
    plain_inline_localstatic_base: u8,
    skipped_static_inline_label_base: u8,
    enum_min: bool,
) -> Compilation<TranslationUnit> {
    // East pointee qualifiers are codegen-transparent, but C++ `const`
    // remains part of a function's ABI name. Move that fact after the star as
    // a parser-internal marker: declaration lookahead keeps seeing canonical
    // `T*`, while parse_type consumes the marker before the declarator name.
    let mut tokens = cxx::normalize_linkage_specifications(tokens);
    tokens = cxx::normalize_constructor_declarators(tokens);
    let mut index = 0;
    while index + 1 < tokens.len() {
        let qualifier = match &tokens[index].token {
            Token::Identifier(word) if word == "const" => Some(true),
            Token::Identifier(word) if word == "volatile" => Some(false),
            _ => None,
        };
        if qualifier.is_some() && tokens[index + 1].token == Token::Star {
            if cplusplus && qualifier == Some(true) {
                tokens.swap(index, index + 1);
                tokens[index + 1].token =
                    Token::Identifier(CXX_POINTEE_CONST_MARKER.to_string());
                index += 2;
            } else {
                tokens.remove(index);
            }
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
        c_linkage_functions: std::collections::HashSet::new(),
        section_functions: std::collections::HashMap::new(),
        section_prototype_order: Vec::new(),
        skipped_inline_names: std::collections::HashSet::new(),
        skipped_inline_definitions: Vec::new(),
        recover_skipped_inline_definition: false,
        inline_bodies: std::collections::HashMap::new(),
        cxx_delete_forwarder: None,
        default_cplusplus: cplusplus,
        cplusplus,
        cplusplus_stack: Vec::new(),
        namespace_stack: Vec::new(),
        cxx_namespaces: std::collections::HashSet::new(),
        current_cxx_layout_scope: None,
        current_member_scope: None,
        force_active: false,
        peephole_disabled: false,
        structs: HashMap::new(),
        cxx_classes: HashMap::new(),
        cxx_class_declaration_order: Vec::new(),
        struct_templates: HashMap::new(),
        template_instantiation_stack: std::cell::RefCell::new(Vec::new()),
        inline_template_members: std::collections::HashSet::new(),
        empty_nested_template_types: std::collections::HashSet::new(),
        inline_cxx_members: std::collections::HashSet::new(),
        cxx_inline_materializations: Vec::new(),
        cxx_static_methods: HashMap::new(),
        cxx_constructors: HashMap::new(),
        cxx_free_functions: HashMap::new(),
        cxx_instance_methods: HashMap::new(),
        cxx_explicit_instance_methods: HashMap::new(),
        cxx_primary_bases: HashMap::new(),
        current_cxx_member_class: None,
        cxx_member_template_forwarders: HashMap::new(),
        cxx_template_forwarder_specializations: HashMap::new(),
        cxx_dispatch_tables: HashMap::new(),
        cxx_virtual_destructor_classes: std::collections::HashSet::new(),
        counted_nested_virtual_positions: std::collections::HashSet::new(),
        cxx_template_virtual_methods: HashMap::new(),
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
        function_parameter_structs: HashMap::new(),
        last_struct_tag: None,
        last_enum_tag: None,
        last_type_was_wchar: false,
        last_source_fundamental: None,
        last_type_was_aggregate_reference: false,
        asm_parameters: Vec::new(),
        expression_struct_tag: None,
        typedefs: HashMap::new(),
        typedef_source_fundamentals: HashMap::new(),
        function_pointer_typedefs: HashMap::new(),
        last_type_was_const: false,
        last_pointer_const: false,
        last_cxx_pointer_depth: 0,
        last_cxx_pointer_base: None,
        last_cxx_function_type: None,
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
        enum_types: HashMap::new(),
        enum_min,
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
    use mwcc_syntax_trees::Expression;

    #[test]
    fn retains_volatile_automatic_storage() {
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(
                "int reload(int value) { volatile int current; current = value; return current; }",
            )
            .unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();

        assert!(unit.functions[0].locals[0].is_volatile);
    }

    #[test]
    fn parses_cpp_named_static_cast_as_an_ordinary_conversion() {
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(
                "float convert(unsigned value) { return static_cast<float>(value); }",
            )
            .unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();

        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Cast {
                target_type: mwcc_syntax_trees::Type::Float,
                ..
            })
        ));
    }

    #[test]
    fn recovers_nested_class_layout_for_out_of_class_member_bodies() {
        let source = r#"
            class Outer {
            public:
                class Inner {
                public:
                    Inner() : wide(0), ratio(0.0f) {}
                    bool initialize();
                private:
                    long long wide;
                    float ratio;
                };
            };
            bool Outer::Inner::initialize() {
                wide = 3;
                ratio = 1.0f;
                return true;
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
            unit.functions[0].parameters.as_slice(),
            [mwcc_syntax_trees::Parameter {
                parameter_type: mwcc_syntax_trees::Type::StructPointer { element_size: 16 },
                ..
            }]
        ));
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [
                mwcc_syntax_trees::Statement::Store {
                    target: mwcc_syntax_trees::Expression::Member {
                        offset: 0,
                        member_type: mwcc_syntax_trees::Type::LongLong,
                        ..
                    },
                    ..
                },
                mwcc_syntax_trees::Statement::Store {
                    target: mwcc_syntax_trees::Expression::Member {
                        offset: 8,
                        member_type: mwcc_syntax_trees::Type::Float,
                        ..
                    },
                    ..
                }
            ]
        ));
    }

    #[test]
    fn lays_out_a_nested_class_with_a_qualified_base() {
        let source = r#"
            struct Collision {
                struct tri_data {
                    unsigned index;
                    float radius;
                    float distance;
                };
                tri_data tri;
            };
            struct Drive {
                struct tri_data : Collision::tri_data {
                    float x;
                };
                unsigned flags;
                float time;
                tri_data tri;
            };
            float read_x(Drive* drive) { return drive->tri.x; }
            void copy_tri(Drive* drive, Collision* collision) {
                *(Collision::tri_data*)&drive->tri = collision->tri;
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

        assert_eq!(unit.aggregate_definitions["Drive"].byte_size, 24);
        assert_eq!(
            unit.aggregate_definitions["Drive::tri_data"].byte_size,
            16
        );
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Member {
                offset: 20,
                ..
            })
        ));
        assert!(matches!(
            unit.functions[1].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Dereference { pointer },
                ..
            }] if matches!(pointer.as_ref(), mwcc_syntax_trees::Expression::Cast { .. })
        ));
    }

    #[test]
    fn enum_min_uses_value_range_for_typedef_and_struct_members() {
        let source = b"\
            typedef enum Kind { Zero = 0, Five = 5 } Kind;\n\
            struct Event { Kind kind; unsigned id; };\n\
            void set(struct Event* event, Kind kind) { event->kind = kind; }\n";
        let unit = parse_located_translation_unit_with_enum_min(
            mwcc_source_to_tokens::tokenize_bytes_located(source).unwrap(),
            false,
            true,
            1,
            3,
            true,
        )
        .unwrap();

        assert_eq!(
            unit.functions[0].parameters[1].parameter_type,
            mwcc_syntax_trees::Type::UnsignedChar
        );
        let mwcc_syntax_trees::Statement::Store {
            target: mwcc_syntax_trees::Expression::Member { member_type, .. },
            ..
        } = &unit.functions[0].statements[0]
        else {
            panic!("expected a member store");
        };
        assert_eq!(*member_type, mwcc_syntax_trees::Type::UnsignedChar);
    }

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
        let mwcc_syntax_trees::Statement::If { then_body, .. } = &unit.functions[0].statements[0]
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
    fn preserves_fixed_address_struct_pointer_indirection() {
        let source = r#"
            typedef struct Context { int state; } Context;
            Context* CURRENT : 0x800000D4;
            Context* get(void) { return (Context*)CURRENT; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.fixed_address_objects.get("CURRENT"), Some(&0x800000D4));
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Cast {
                target_type: mwcc_syntax_trees::Type::StructPointer { element_size: 4 },
                operand,
            }) if matches!(
                operand.as_ref(),
                mwcc_syntax_trees::Expression::Dereference { pointer }
                    if matches!(
                        pointer.as_ref(),
                        mwcc_syntax_trees::Expression::Cast {
                            target_type: mwcc_syntax_trees::Type::Pointer(
                                mwcc_syntax_trees::Pointee::Pointer
                            ),
                            operand,
                        } if matches!(
                            operand.as_ref(),
                            mwcc_syntax_trees::Expression::IntegerLiteral(0x800000D4)
                        )
                    )
            )
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
    fn retains_section_attributes_on_asm_prototypes() {
        let source = r#"
            __declspec(section ".init") asm void early(void);
            __declspec(section ".init") void asm later(void* address);
            void body(void) {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.section_prototypes, ["early", "later"]);
        assert_eq!(unit.prototypes.len(), 0);
        assert_eq!(unit.functions.len(), 1);
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
    fn resolves_indexed_asm_parameter_members() {
        let source = r#"
            typedef struct Words { unsigned int values[4]; } Words;
            typedef struct Context { int prefix; Words registers; } Context;
            asm void save(register Context* context) {
                nofralloc
                lwz r3, context->registers.values[2]
                stw r3, context->registers.values[3]
                ori r4, r4, 0x8000 | 0x20 | 0x2
                lwz r5, (r4)
                stw r5, current_context@l(r4)
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
                    == mwcc_syntax_trees::AsmOperand::Memory { displacement: 12, base: 3 }
        ));
        assert!(matches!(
            &body[2],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[1]
                    == mwcc_syntax_trees::AsmOperand::Memory { displacement: 16, base: 3 }
        ));
        assert!(matches!(
            &body[3],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[2]
                    == mwcc_syntax_trees::AsmOperand::Immediate(0x8022)
        ));
        assert!(matches!(
            &body[4],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[1]
                    == mwcc_syntax_trees::AsmOperand::Memory { displacement: 0, base: 4 }
        ));
        assert!(matches!(
            &body[5],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[1]
                    == mwcc_syntax_trees::AsmOperand::SymbolMemory {
                        name: "current_context".to_string(),
                        suffix: mwcc_syntax_trees::AsmRelocSuffix::Lo,
                        base: 4,
                    }
        ));
    }

    #[test]
    fn folds_bitwise_complemented_asm_immediates() {
        let source = r#"
            asm void flush(void) {
                nofralloc
                lis r5, ~0
                ori r5, r5, ~14
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
                if instruction.operands[1] == mwcc_syntax_trees::AsmOperand::Immediate(-1)
        ));
        assert!(matches!(
            &body[2],
            mwcc_syntax_trees::AsmItem::Instruction(instruction)
                if instruction.operands[2] == mwcc_syntax_trees::AsmOperand::Immediate(-15)
        ));
    }

    #[test]
    fn retains_asm_section_and_prior_asm_fact() {
        let source = r#"
            extern "C" {
                __declspec(section ".init") asm void startup(void) {
                    nofralloc
                    blr
                }
                void after_startup(void) {}
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
        assert_eq!(unit.functions[0].section.as_deref(), Some(".init"));
        assert!(unit.functions[1].preceded_by_asm);
    }

    #[test]
    fn retains_sections_on_extern_function_pointer_arrays() {
        let source = r#"
            typedef void (*callback)(void);
            __declspec(section ".ctors") extern callback _ctors[];
            __declspec(section ".dtors") extern callback _dtors[];
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.globals[0].section.as_deref(), Some(".ctors"));
        assert_eq!(unit.globals[1].section.as_deref(), Some(".dtors"));
        assert!(unit.globals.iter().all(|global| global.is_extern));
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
                local_lines: Vec::new(),
                statement_lines: Vec::new(),
                terminal_return_line: Some(3),
                body_end_line: 4,
            })]
        );
    }

    #[test]
    fn retains_typed_asm_parameters_and_instruction_lines() {
        let source = b"typedef struct Record { int value; } Record;\n\
asm void load(register Record* record) {\n\
nofralloc\n\
lwz r3, Record.value(record)\n\
blr\n\
}\n";
        let unit = parse_located_translation_unit(
            mwcc_source_to_tokens::tokenize_bytes_located(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();

        assert_eq!(unit.functions[0].parameters.len(), 1);
        assert_eq!(unit.functions[0].parameters[0].name, "record");
        assert_eq!(
            unit.functions[0].parameters[0].parameter_type,
            mwcc_syntax_trees::Type::StructPointer { element_size: 4 }
        );
        assert_eq!(
            unit.function_parameter_aggregate_tags
                .get(&("load".to_string(), "record".to_string())),
            Some(&"Record".to_string())
        );
        let instruction_lines = unit.functions[0]
            .asm_body
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(|item| match item {
                mwcc_syntax_trees::AsmItem::Instruction(instruction) => {
                    Some(instruction.source_line)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(instruction_lines, [3, 4, 5]);
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
    fn retains_skipped_inline_definitions_for_semantic_analysis() {
        let source = r#"
            static inline int append_one(unsigned char *p, unsigned char v) {
                if (*p) return 7;
                *p = v;
                return 0;
            }
            int compiled(unsigned char *p) { return append_one(p, 3); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "compiled");
        assert!(unit.skipped_inline_names.contains("append_one"));
        assert_eq!(unit.skipped_inline_definitions.len(), 1);
        assert_eq!(unit.skipped_inline_definitions[0].name, "append_one");
        assert_eq!(unit.skipped_inline_definitions[0].statements.len(), 2);
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
    fn retains_source_aggregate_graph_for_debug_lowering() {
        let source = r#"
            typedef unsigned char u8;
            typedef short s16;
            typedef unsigned long u32;
            typedef struct animation_s {
                u8* flag_table;
                s16* data_table;
                s16* key_table;
                s16* fixed_table;
                s16 pad;
                s16 frames;
            } Animation;
            typedef struct {
                u32* words;
            } Anonymous;
            u8 flags[] = { 0, 1 };
            Animation animation = { flags, 0, 0, 0, -1, 11 };
            Anonymous anonymous = { 0 };
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            3,
            3,
        )
        .unwrap();

        assert_eq!(unit.global_aggregate_tags["animation"], "animation_s");
        let aggregate = &unit.aggregate_definitions["animation_s"];
        assert_eq!(aggregate.source_tag.as_deref(), Some("animation_s"));
        assert_eq!(aggregate.byte_size, 20);
        assert_eq!(
            aggregate
                .members
                .iter()
                .map(|member| (member.name.as_str(), member.declared_type, member.offset))
                .collect::<Vec<_>>(),
            [
                (
                    "flag_table",
                    mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::UnsignedChar,),
                    0,
                ),
                (
                    "data_table",
                    mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Short),
                    4,
                ),
                (
                    "key_table",
                    mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Short),
                    8,
                ),
                (
                    "fixed_table",
                    mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Short),
                    12,
                ),
                ("pad", mwcc_syntax_trees::Type::Short, 16),
                ("frames", mwcc_syntax_trees::Type::Short, 18),
            ]
        );
        let anonymous = &unit.aggregate_definitions["Anonymous"];
        assert_eq!(anonymous.source_tag, None);
        assert_eq!(
            anonymous.members[0].source_fundamental,
            Some(mwcc_syntax_trees::SourceFundamentalType::UnsignedLong)
        );
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
            .any(|statement| matches!(
                statement,
                mwcc_syntax_trees::Statement::Loop {
                    kind: mwcc_syntax_trees::LoopKind::For,
                    initializer: Some(mwcc_syntax_trees::Expression::Assign { .. }),
                    ..
                }
            )));
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
    fn lowers_an_implicit_virtual_member_call_through_its_vtable_slot() {
        let source = r#"
            class Bank {
            public:
                virtual unsigned count() const = 0;
                int loading();
            };
            int Bank::loading() { return count(); }
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
                object,
                vptr_offset: 0,
                slot_offset: 8,
                arguments,
                ..
            }) if matches!(object.as_ref(), mwcc_syntax_trees::Expression::Variable(name) if name == "this")
                && arguments.is_empty()
        ));
    }

    #[test]
    fn preserves_const_qualification_on_a_direct_instance_call() {
        let source = r#"
            class Item {
            public:
                int ready() const;
            };
            int check(Item* item) { return item->ready(); }
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
            Some(mwcc_syntax_trees::Expression::Call { name, .. })
                if name == "ready__4ItemCFv"
        ));
    }

    #[test]
    fn resolves_duplicate_class_names_in_the_active_namespace() {
        let source = r#"
            namespace First {
                struct Obj { int run(int); };
                int call(Obj* object) { return object->run(1); }
            }
            namespace Second {
                struct Obj { int run(int); };
                int call(Obj* object) { return object->run(2); }
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
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. })
                if name == "run__Q25First3ObjFi"
        ));
        assert!(matches!(
            unit.functions[1].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. })
                if name == "run__Q26Second3ObjFi"
        ));
    }

    #[test]
    fn keeps_duplicate_class_layouts_qualified_across_reopened_namespaces() {
        let source = r#"
            namespace First {
                struct Obj { int first; int read(); };
            }
            namespace Second {
                struct Obj { int padding; int second; int read(); };
            }
            namespace First {
                int Obj::read() { return first; }
            }
            namespace Second {
                int Obj::read() { return second; }
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
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, .. })
        ));
        assert!(matches!(
            unit.functions[1].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Member { offset: 4, .. })
        ));
    }

    #[test]
    fn mangles_free_cpp_functions_and_preserves_c_linkage() {
        let source = r#"
            extern "C" { int c_api(float); }
            int c_api(float value) { return 1; }
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
            ["c_api", "cpp_api__Ff", "caller__Ff", "used__2IdCFv"]
        );
        assert!(matches!(
            unit.functions[1].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "c_api"
        ));
        assert!(matches!(
            unit.functions[2].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. }) if name == "cpp_api__Ff"
        ));
    }

    #[test]
    fn resolves_namespace_qualified_free_function_calls_and_definitions() {
        let source = r#"
            extern "C" { float sinf(float value); }
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
        assert_eq!(
            destructor.parameters[1].parameter_type,
            mwcc_syntax_trees::Type::Short
        );
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
    fn first_out_of_line_virtual_method_owns_the_class_vtable() {
        let source = r#"
            class Reader {
            public:
                virtual int first();
                virtual int second();
            };
            int Reader::first() { return 1; }
            int Reader::second() { return 2; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();

        let vtable = unit
            .globals
            .iter()
            .find(|global| global.name == "__vt__6Reader")
            .expect("the first ordinary virtual definition owns the class vtable");
        assert_eq!(vtable.data_bytes.as_ref().map(Vec::len), Some(16));
        assert_eq!(
            vtable.data_relocations,
            vec![
                (8, "first__6ReaderFv".to_string(), 0),
                (12, "second__6ReaderFv".to_string(), 0),
            ]
        );
        assert_eq!(unit.cxx_inline_ordinal_facts.virtual_method_declarations, 2);
        assert_eq!(
            unit.cxx_inline_ordinal_facts
                .virtual_destructor_declarations,
            0
        );
    }

    #[test]
    fn ordinary_key_function_vtable_includes_a_later_virtual_destructor() {
        let source = r#"
            class Reader {
            public:
                virtual int value();
                virtual ~Reader();
            };
            int Reader::value() { return 1; }
            Reader::~Reader() {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let vtable = unit
            .globals
            .iter()
            .find(|global| global.name == "__vt__6Reader")
            .unwrap();
        assert_eq!(
            vtable.data_relocations,
            vec![
                (12, "__dt__6ReaderFv".to_string(), 0),
                (8, "value__6ReaderFv".to_string(), 0),
            ]
        );
        assert_eq!(vtable.functions_before, 2);
        assert_eq!(unit.cxx_inline_ordinal_facts.virtual_method_declarations, 1);
        assert_eq!(
            unit.cxx_inline_ordinal_facts
                .virtual_destructor_declarations,
            1
        );
    }

    #[test]
    fn earlier_virtual_destructor_remains_the_key_function() {
        let source = r#"
            class Reader {
            public:
                virtual ~Reader();
                virtual int value();
            };
            int Reader::value() { return 1; }
            Reader::~Reader() {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let vtable = unit
            .globals
            .iter()
            .find(|global| global.name == "__vt__6Reader")
            .unwrap();
        assert_eq!(vtable.functions_before, 2);
    }

    #[test]
    fn inlines_a_scalar_delete_forwarder_into_a_virtual_destructor() {
        let source = r#"
            class Memory {
            public:
                static void Free(const void* pointer);
            };
            inline void operator delete(void* pointer) { Memory::Free(pointer); }
            class Binder {
            public:
                virtual ~Binder();
            };
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
        let destructor = unit
            .functions
            .iter()
            .find(|function| function.name == "__dt__6BinderFv")
            .unwrap();
        let mwcc_syntax_trees::Statement::If { then_body, .. } = &destructor.statements[0] else {
            panic!("expected the synthesized destructor guard");
        };
        let mwcc_syntax_trees::Statement::If { then_body, .. } = &then_body[1] else {
            panic!("expected the deleting guard");
        };
        assert!(matches!(
            &then_body[0],
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name, .. }
            ) if name == "Free__6MemoryFPCv"
        ));
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
                virtual_method_declarations: 0,
                virtual_destructor_declarations: 1,
                inherited_virtual_destructor_declarations: 0,
                direct_calls: 1,
                control_flow_labels: 0,
            }
        );
    }

    #[test]
    fn rtti_analysis_counts_implicit_overrides_and_nested_classes() {
        let override_source = r#"
            class Base {
            public:
                virtual int value();
            };
            class Derived : public Base {
            public:
                int value();
            };
            int Base::value() { return 1; }
            int Derived::value() { return 2; }
        "#;
        let override_unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(override_source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            override_unit
                .cxx_inline_ordinal_facts
                .virtual_method_declarations,
            2
        );

        let nested_source = r#"
            class Outer {
            public:
                class Nested {
                public:
                    virtual int value();
                };
            };
            int Outer::Nested::value() { return 1; }
        "#;
        let nested_unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(nested_source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(
            nested_unit
                .cxx_inline_ordinal_facts
                .virtual_method_declarations,
            1
        );
    }

    #[test]
    fn rtti_analysis_distinguishes_inherited_virtual_destructors() {
        let source = r#"
            class Base {
            public:
                virtual ~Base();
            };
            class Derived : public Base {
            public:
                ~Derived();
            };
            Base::~Base() {}
            Derived::~Derived() {}
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
            unit.cxx_inline_ordinal_facts
                .virtual_destructor_declarations,
            2
        );
        assert_eq!(
            unit.cxx_inline_ordinal_facts
                .inherited_virtual_destructor_declarations,
            1
        );
    }

    #[test]
    fn counts_control_flow_inside_dropped_in_class_definitions() {
        let source = r#"
            class Timer {
            public:
                void reset(int value) {
                    if (value) value = 0;
                }
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

        assert_eq!(unit.cxx_inline_ordinal_facts.control_flow_labels, 2);
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
    fn preserves_multidimensional_member_row_stride_in_address_expression() {
        let source = r#"
            typedef unsigned char u8;
            struct State { u8 prefix[2]; u8 flag[3][16]; };
            struct State state;
            u8 *probe(unsigned group) { return &state.flag[group][0]; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        let Some(Expression::AddressOf { operand }) = &unit.functions[0].return_expression else {
            panic!("expected address expression")
        };
        let Expression::Index { base: row, .. } = operand.as_ref() else {
            panic!("expected column index")
        };
        let Expression::Index { base: member, .. } = row.as_ref() else {
            panic!("expected row index")
        };
        let Expression::MemberAddress { index_stride, .. } = member.as_ref() else {
            panic!("expected member-array address")
        };
        assert_eq!(*index_stride, Some(16));
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
    fn lowers_base_qualified_virtual_calls_as_direct_calls_with_this() {
        let source = r#"
            namespace Game {
                struct EnemyBase {
                    virtual void setParameters(int);
                };
                namespace Actor {
                    struct Obj : public EnemyBase {
                        void configure();
                    };
                    void Obj::configure() { EnemyBase::setParameters(3); }
                }
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
                mwcc_syntax_trees::Expression::Call { name, arguments }
            )] if name == "setParameters__Q24Game9EnemyBaseFi"
                && matches!(arguments.as_slice(), [
                    mwcc_syntax_trees::Expression::Variable(this),
                    mwcc_syntax_trees::Expression::IntegerLiteral(3),
                ] if this == "this")
        ));
    }

    #[test]
    fn resolves_an_inherited_direct_call_through_a_primary_base() {
        let source = r#"
            struct Point {};
            struct Primary { void Set2(Point const*, Point const*, unsigned int); };
            struct Secondary {};
            struct Check : public Primary, public Secondary { void Set(Point const*, Point const*, unsigned int); };
            void Check::Set(Point const* start, Point const* end, unsigned int id) { Set2(start, end, id); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(), true, true, 1, 3,
        ).unwrap();
        assert!(matches!(unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name, arguments }
            )] if name == "Set2__7PrimaryFPC5PointPC5PointUi"
                && matches!(arguments.as_slice(), [
                    mwcc_syntax_trees::Expression::Variable(this),
                    mwcc_syntax_trees::Expression::Variable(start),
                    mwcc_syntax_trees::Expression::Variable(end),
                    mwcc_syntax_trees::Expression::Variable(id),
                ] if this == "this" && start == "start" && end == "end" && id == "id")));
    }

    #[test]
    fn adjusts_this_for_an_inherited_secondary_base_call() {
        let source = r#"
            struct Primary { int first; };
            struct Secondary { int second; void inspect(); };
            struct Derived : public Primary, public Secondary { void run(); };
            void Derived::run() { inspect(); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(), true, true, 1, 3,
        ).unwrap();
        assert!(matches!(unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name, arguments }
            )] if name == "inspect__9SecondaryFv"
                && matches!(arguments.as_slice(), [
                    mwcc_syntax_trees::Expression::MemberAddress { base, offset: 4, .. }
                ] if matches!(base.as_ref(), mwcc_syntax_trees::Expression::Variable(this) if this == "this"))));
    }

    #[test]
    fn lays_out_multiple_bases_and_synthesizes_adjusted_default_constructor_calls() {
        let source = r#"
            struct Primary { Primary(); int first; };
            struct Secondary { Secondary(); int second; };
            struct Derived : public Primary, public Secondary { Derived(); ; ; };
            Derived::Derived() {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(), true, true, 1, 3,
        ).unwrap();
        let function = &unit.functions[0];
        assert_eq!(function.name, "__ct__7DerivedFv");
        assert!(matches!(function.statements.as_slice(), [
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: primary, arguments: primary_arguments }
            ),
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: secondary, arguments: secondary_arguments }
            ),
        ] if primary == "__ct__7PrimaryFv"
            && matches!(primary_arguments.as_slice(), [mwcc_syntax_trees::Expression::Variable(this)] if this == "this")
            && secondary == "__ct__9SecondaryFv"
            && matches!(secondary_arguments.as_slice(), [mwcc_syntax_trees::Expression::MemberAddress { base, offset: 4, .. }]
                if matches!(base.as_ref(), mwcc_syntax_trees::Expression::Variable(this) if this == "this"))));
        assert!(matches!(function.return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Variable(this)) if this == "this"));
    }

    #[test]
    fn groups_inherited_vptrs_after_base_construction() {
        let source = r#"
            class Primary { int first; public: Primary(); virtual ~Primary(); };
            class Secondary { int second; public: Secondary(); virtual ~Secondary(); };
            class Derived : public Primary, public Secondary {
            public:
                Derived();
                virtual ~Derived();
            };
            Derived::Derived() {}
            Derived::~Derived() {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(), true, true, 1, 3,
        ).unwrap();
        assert_eq!(
            unit.cxx_class_declaration_order,
            ["Primary", "Secondary", "Derived"]
        );
        let constructor = &unit.functions[0];
        assert!(matches!(constructor.statements.as_slice(), [
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: primary, .. }
            ),
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: secondary, .. }
            ),
            mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Member { offset: 4, .. },
                value: mwcc_syntax_trees::Expression::AddressOf { operand },
            },
            mwcc_syntax_trees::Statement::Store {
                target: mwcc_syntax_trees::Expression::Member { offset: 12, .. },
                value: mwcc_syntax_trees::Expression::MemberAddress {
                    base,
                    offset: 12,
                    ..
                },
            },
        ] if primary == "__ct__7PrimaryFv"
            && secondary == "__ct__9SecondaryFv"
            && matches!(operand.as_ref(), mwcc_syntax_trees::Expression::Variable(vtable)
                if vtable == "__vt__7Derived")
            && matches!(base.as_ref(), mwcc_syntax_trees::Expression::AddressOf { operand }
                if matches!(operand.as_ref(), mwcc_syntax_trees::Expression::Variable(vtable)
                    if vtable == "__vt__7Derived"))));
        let vtable = unit.globals.iter().find(|global| global.name == "__vt__7Derived")
            .expect("the derived destructor owns the complete vtable group");
        assert_eq!(vtable.data_bytes.as_ref().map(Vec::len), Some(24));
        assert_eq!(vtable.data_relocations, vec![
            (8, "__dt__7DerivedFv".to_string(), 0),
            (20, "@8@__dt__7DerivedFv".to_string(), 0),
        ]);
        assert!(matches!(unit.functions[1].statements.as_slice(), [
            mwcc_syntax_trees::Statement::If { then_body, .. }
        ] if matches!(then_body.as_slice(), [
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: secondary, arguments: secondary_args }
            ),
            mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { name: primary, arguments: primary_args }
            ),
            mwcc_syntax_trees::Statement::If { .. },
        ] if secondary == "__dt__9SecondaryFv"
            && matches!(secondary_args.as_slice(), [
                mwcc_syntax_trees::Expression::MemberAddress { offset: 8, .. },
                mwcc_syntax_trees::Expression::IntegerLiteral(0),
            ])
            && primary == "__dt__7PrimaryFv"
            && matches!(primary_args.as_slice(), [
                mwcc_syntax_trees::Expression::Variable(this),
                mwcc_syntax_trees::Expression::IntegerLiteral(0),
            ] if this == "this"))));
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
    fn resolves_an_explicit_member_template_through_a_specialized_forwarder() {
        let source = r#"
            typedef unsigned int uint;
            template <typename T> struct TType {};

            class Stream {
            public:
                uint ReadLong(void);
                template <typename T>
                T Get(const TType<T>& type = TType<T>()) {
                    return helper(TType<T>(), *this);
                }
            };

            template <typename T>
            T helper(const TType<T>&, Stream&);
            template <>
            inline uint helper(const TType<uint>&, Stream& input) {
                return input.ReadLong();
            }

            uint read(Stream& input) { return input.Get<uint>(); }
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
            Some(mwcc_syntax_trees::Expression::Call { name, arguments })
                if name == "ReadLong__6StreamFv" && arguments.len() == 1
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
    fn retains_function_pointer_typedef_signatures_in_free_function_mangling() {
        let source = r#"
            #pragma cplusplus on
            typedef int (*Callback)(void*, void*);
            extern int invoke(short kind, Callback callback, void* context);
            int caller(void) { return invoke(1, 0, 0); }
            #pragma cplusplus off
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.prototypes[0].0, "invoke__FsPFPvPv_iPv");
        assert!(matches!(
            unit.functions[0].return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Call { name, .. })
                if name == "invoke__FsPFPvPv_iPv"
        ));
    }

    #[test]
    fn preserves_a_function_pointer_through_a_typedef_alias() {
        let source = r#"
            typedef void (*InterruptHandler)(int, void*);
            typedef InterruptHandler MonitorCallback;
            void initialize(void* state, MonitorCallback callback) { }
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
            unit.functions[0].parameters.as_slice(),
            [_, mwcc_syntax_trees::Parameter {
                parameter_type: mwcc_syntax_trees::Type::Pointer(
                    mwcc_syntax_trees::Pointee::Int
                ),
                ..
            }]
        ));
    }

    #[test]
    fn mangles_qualified_template_pointer_arguments_with_qualified_pointees() {
        let source = r#"
            namespace zen {
                struct particleMdl;
                template <typename A>
                struct CallBack1 { virtual bool invoke(A) = 0; };
            }
            int use(zen::CallBack1<zen::particleMdl*>* callback) { return 0; }
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
            unit.functions[0].name,
            "use__FPQ23zen31CallBack1<PQ23zen11particleMdl>"
        );
    }

    #[test]
    fn rejects_opaque_template_specializations_passed_by_value() {
        let source = r#"
            namespace zen {
                struct particleMdl;
                template <typename A>
                struct CallBack1 { virtual bool invoke(A) = 0; };
            }
            int use(zen::CallBack1<zen::particleMdl*> callback) { return 0; }
        "#;
        assert!(parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .is_err());
    }

    #[test]
    fn retains_in_class_inline_member_bodies_for_semantic_inlining() {
        let source = r#"
            struct Box {
                int value;
                int get() { return value; }
            };
            int use(Box* box) { return box->get(); }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(unit.skipped_inline_names.contains("get__3BoxFv"));
        assert_eq!(unit.skipped_inline_definitions.len(), 1);
        assert_eq!(unit.skipped_inline_definitions[0].name, "get__3BoxFv");
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(Expression::Call { ref name, .. }) if name == "get__3BoxFv"
        ));
    }

    #[test]
    fn resolves_virtual_calls_through_opaque_template_specializations() {
        let source = r#"
            namespace api {
                struct Item;
                template <typename A>
                struct Callback { virtual bool invoke(A) = 0; };
            }
            int use(api::Callback<api::Item*>* callback, api::Item* item) {
                return callback->invoke(item);
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
            unit.functions[0].return_expression,
            Some(Expression::VirtualCall {
                vptr_offset: 0,
                slot_offset: 8,
                ..
            })
        ));
    }

    #[test]
    fn retains_block_local_constructor_calls_for_inline_expansion() {
        let source = r#"
            struct Pixel {
                Pixel(int value) { x = value; }
                int x;
            };
            int use(int value) {
                if (value) {
                    Pixel pixel(value);
                }
                return value;
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
        assert!(unit.skipped_inline_names.contains("__ct__5PixelFi"));
        assert!(unit
            .skipped_inline_definitions
            .iter()
            .any(|function| function.name == "__ct__5PixelFi"));
        assert!(matches!(
            unit.functions[0].statements.as_slice(),
            [mwcc_syntax_trees::Statement::If { then_body, .. }]
                if matches!(then_body.as_slice(),
                    [mwcc_syntax_trees::Statement::Expression(Expression::Call { name, .. })]
                    if name == "__ct__5PixelFi")
        ));
    }

    #[test]
    fn resolves_local_typeof_aliases_for_anonymous_member_elements() {
        let source = r#"
            struct Asset {
                struct { short x; short y; } positions[2];
            };
            int read(struct Asset* asset) {
                typedef __typeof__(((struct Asset){ 0 }).positions[0]) position_t;
                position_t* position = &asset->positions[0];
                return position->y;
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
            unit.functions[0].return_expression,
            Some(Expression::Member { offset: 2, .. })
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
    fn recognizes_pointer_template_instances_as_local_declarations() {
        let source = r#"
            template <typename T> struct Box { T value; };
            struct Item { int value; };
            int present(Box<Item*>* box) {
                Box<Item*>* local = box;
                return local != 0;
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
        let local = &unit.functions[0].locals[0];
        assert_eq!(local.name, "local");
        assert_eq!(
            local.declared_type,
            mwcc_syntax_trees::Type::StructPointer { element_size: 4 }
        );
    }

    #[test]
    fn recovers_self_pointer_fields_in_template_instances() {
        let source = r#"
            template <typename T> class Node {
            public:
                Node<T>* next;
                Node<T>* previous;
                T data;
            };
            int read(Node<int>* node) { return node->next->data; }
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
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn excludes_static_self_values_from_template_instance_layouts() {
        let source = r#"
            template <typename T> struct Vector3 {
                T x, y, z;
                static Vector3<T> zero;
            };
            int read_z(Vector3<int>* value) { return value->z; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert_eq!(unit.functions[0].name, "read_z__FP10Vector3<i>");
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn bounds_recursive_template_value_layout_recovery() {
        let source = r#"
            template <typename T> struct Recursive {
                Recursive<T> value;
            };
            Recursive<int> instance;
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(unit.globals.is_empty());
    }

    #[test]
    fn recovers_nested_template_base_and_parameter_value_layouts() {
        let source = r#"
            class Allocator { public: int initial; int delta; };
            template <typename T, typename Adapter> class Container {
            public:
                Adapter allocator;
                T* head;
            };
            template <typename T> class Pool : public Container<T, Allocator> {};
            class Manager { public: Pool<int> active; int read(); };
            int Manager::read() { return active.allocator.delta + *active.head; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(unit.functions[0].return_expression.is_some());
    }

    #[test]
    fn recovers_class_layout_with_function_pointer_constructor_parameters() {
        let source = r#"
            class Tween {
            public:
                Tween(float (*curve)(float, float), void (*apply)(void*, float*));
                int active;
            };
            int read(Tween* tween) { return tween->active; }
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
        assert!(matches!(
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, .. })
        ));
    }

    #[test]
    fn recovers_class_array_member_layout_and_indexing() {
        let source = r#"
            class Tween {
            public:
                float start[4];
                float grid[2][3];
                int tail;
            };
            float read(Tween* tween, int row, int column) {
                return tween->start[column] + tween->grid[row][column] + tween->tail;
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
        let expression = unit.functions[0].return_expression.as_ref().unwrap();
        let rendered = format!("{expression:?}");
        assert!(rendered.contains("offset: 0"));
        assert!(rendered.contains("offset: 16"));
        assert!(rendered.contains("index_stride: Some(12)"));
        assert!(rendered.contains("offset: 40"));
    }

    #[test]
    fn recovers_function_pointer_class_member_layout() {
        let source = r#"
            class Handler {
            public:
                int active;
                void (*done)(void*);
                int tail;
            };
            int read(Handler* handler) { return handler->tail; }
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
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn normalizes_scalar_delete_to_a_virtual_deleting_destructor_call() {
        let source = r#"
            class Item { public: virtual ~Item(); };
            void destroy(Item* item) { delete item; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let mwcc_syntax_trees::Statement::If { then_body, .. } = &unit.functions[0].statements[0]
        else {
            panic!("expected the delete null guard");
        };
        assert!(matches!(
            then_body.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::VirtualCall {
                    slot_offset: 8,
                    arguments,
                    ..
                }
            )] if matches!(arguments.as_slice(), [mwcc_syntax_trees::Expression::IntegerLiteral(-1)])
        ));
    }

    #[test]
    fn normalizes_placement_new_to_a_pointer_returning_constructor_call() {
        let source = r#"
            void* allocate(int);
            class Item { public: Item(int); int value; };
            Item* create(int value) { return new (allocate(16)) Item(value); }
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
            Some(mwcc_syntax_trees::Expression::Call { name, arguments })
                if name.starts_with("__ct__4ItemF") && arguments.len() == 2
        ));
    }

    #[test]
    fn normalizes_trivial_scalar_new_to_the_eabi_allocator() {
        let source = r#"
            void* operator new(unsigned long);
            int* create() { return new int; }
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
            Some(mwcc_syntax_trees::Expression::Call { name, arguments })
                if name == "__nw__FUl"
                    && matches!(arguments.as_slice(), [mwcc_syntax_trees::Expression::IntegerLiteral(4)])
        ));
    }

    #[test]
    fn normalizes_trivial_array_new_to_the_eabi_array_allocator() {
        let source = r#"
            void* operator new[](unsigned long);
            char* create() { return new char[64]; }
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
            Some(mwcc_syntax_trees::Expression::Call { name, arguments })
                if name == "__nwa__FUl"
                    && matches!(arguments.as_slice(), [mwcc_syntax_trees::Expression::IntegerLiteral(64)])
        ));
    }

    #[test]
    fn elaborated_class_value_members_retain_their_nested_layout() {
        let source = r#"
            struct Vector { struct { float x; float y; float z; } f; };
            class Owner {
            public:
                virtual ~Owner() { };
                class Vector value;
                float read();
            };
            float Owner::read() { return value.f.z; }
            class Consumer { public: class Vector& get(int) const; };
            float sample(Consumer* consumer) {
                const class Vector& value = consumer->get(0);
                return value.f.z;
            }
            void shadowed(Consumer* Consumer) { Consumer->get(0); }
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
            unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 12, .. })
        ));
        assert!(matches!(
            unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
        assert!(matches!(
            unit.functions[2].statements.as_slice(),
            [mwcc_syntax_trees::Statement::Expression(
                mwcc_syntax_trees::Expression::Call { .. }
            )]
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
            mwcc_syntax_trees::Type::Struct {
                size: 152,
                align: 8
            }
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
            mwcc_syntax_trees::Type::Struct {
                size: 32,
                align: 32
            }
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
            mwcc_syntax_trees::Type::Struct {
                size: 64,
                align: 32
            }
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
    fn retains_struct_layout_across_inline_cxx_operators() {
        let source = r#"
            struct Pixel {
                unsigned char red;
                unsigned char green;
                unsigned char blue;
                unsigned char alpha;
                Pixel& operator=(const Pixel& rhs) {
                    red = rhs.red;
                    green = rhs.green;
                    blue = rhs.blue;
                    alpha = rhs.alpha;
                    return *this;
                };
            };
            unsigned alpha(Pixel* pixel) { return pixel->alpha; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 3, .. })
        ));
    }

    #[test]
    fn retains_class_layout_across_reference_returning_subscript_operators() {
        let source = r#"
            class Vector {
            public:
                enum Kind { First, Second };
                Vector normalized() const { return *this; }
                bool is_equal(const Vector& other, float epsilon = 0.00001f) const;
                float& operator[](int index) { return (&x)[index]; }
                const float& operator[](int index) const { return (&x)[index]; }
            protected:
                float x;
                float y;
                float z;
                Kind kind;
            };
            class Holder { Vector vector; };
            Holder make(void) {}
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
            unit.functions[0].return_type,
            mwcc_syntax_trees::Type::Struct { size: 16, align: 4 }
        );
    }

    #[test]
    fn retains_qualified_nested_enum_identity_without_outer_layout() {
        let source = r#"
            class Particle {
            public:
                enum Mode { Initial, Continuous };
            private:
                Missing value;
            };
            class Reader {
            public:
                virtual Particle::Mode mode(void) const = 0;
            };
        "#;
        parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
    }

    #[test]
    fn materializes_a_vtable_referenced_constant_inline_virtual() {
        let source = r#"
            class Reader {
            public:
                virtual ~Reader();
                virtual bool ready(void) const { return false; }
            };
            Reader::~Reader() {}
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();
        let inline = unit
            .functions
            .iter()
            .find(|function| function.name == "ready__6ReaderCFv")
            .unwrap_or_else(|| panic!("{unit:#?}"));
        assert!(inline.is_weak);
        assert!(matches!(
            inline.return_expression,
            Some(mwcc_syntax_trees::Expression::IntegerLiteral(0))
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
        let command = locals.iter().find(|local| local.name == "command").unwrap();
        let marker = locals.iter().find(|local| local.name == "marker").unwrap();
        assert_eq!(light.data_bytes.as_deref(), Some(&[90, 90, 45, 255][..]));
        assert_eq!(command.data_bytes.as_deref(), Some(&[0, 0, 0, 0][..]));
        assert_eq!(marker.data_bytes.as_deref(), Some(&[1, 0, 0, 0][..]));
        assert!(light.is_static && command.is_static && marker.is_static);
    }

    #[test]
    fn namespace_const_has_internal_linkage_only_in_cxx() {
        let tokens = mwcc_source_to_tokens::tokenize("const int value = 3;").unwrap();
        let cxx = parse_translation_unit(tokens.clone(), true, true, 1, 3).unwrap();
        let c = parse_translation_unit(tokens, false, true, 1, 3).unwrap();

        assert!(cxx.globals[0].is_static);
        assert!(!c.globals[0].is_static);
    }

    #[test]
    fn retains_signed_long_long_width_in_struct_layouts() {
        let source = r#"
            typedef signed long long int OSTime;
            typedef struct Packet {
                int channel;
                void* output;
                unsigned output_bytes;
                void* input;
                unsigned input_bytes;
                void (*callback)(int, unsigned, void*);
                OSTime fire;
            } Packet;
            static Packet packets[4];
            int channel(Packet* packet, int index) {
                return packets[index].channel;
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
            unit.globals[0].declared_type,
            mwcc_syntax_trees::Type::Struct { size: 32, align: 8 }
        ));
        assert!(matches!(
            &unit.functions[0].return_expression,
            Some(mwcc_syntax_trees::Expression::Member {
                index_stride: Some(32),
                ..
            })
        ));
    }

    #[test]
    fn recovers_layout_past_a_pointer_to_array_member() {
        let source = r#"
            struct Packet {
                int before;
                unsigned char (*rows)[2];
                int after;
            };
            int read_after(struct Packet* packet) { return packet->after; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn defers_access_to_an_unmodeled_pointer_to_array_member() {
        let source = r#"
            struct Packet { unsigned char (*rows)[2]; };
            unsigned char* rows(struct Packet* packet) { return packet->rows; }
        "#;
        let error = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap_err();
        assert!(error
            .message
            .contains("accessing pointer-to-array member 'rows' is not supported yet"));
    }

    #[test]
    fn recovers_an_inline_anonymous_struct_pointer_member() {
        let source = r#"
            struct Packet {
                int before;
                struct { int value; }* nested;
                int after;
            };
            int read_nested(struct Packet* packet) { return packet->nested->value; }
            int read_after(struct Packet* packet) { return packet->after; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, base, .. })
                if matches!(base.as_ref(), mwcc_syntax_trees::Expression::Member { offset: 4, .. })
        ));
        assert!(matches!(
            &unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn recovers_layout_past_a_pointer_array_in_an_anonymous_union() {
        let source = r#"
            struct Command {
                float timer;
                float frame;
                union {
                    unsigned* pointers[1];
                    unsigned word;
                };
                int after;
            };
            int read_after(struct Command* command) { return command->after; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 12, .. })
        ));
    }

    #[test]
    fn recovers_a_bit_field_variant_in_an_anonymous_union() {
        let source = r#"
            struct Hit {
                int before;
                union {
                    void* owner;
                    unsigned char flag : 1;
                };
                int after;
            };
            int read_flag(struct Hit* hit) { return hit->flag; }
            int read_after(struct Hit* hit) { return hit->after; }
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
            Some(mwcc_syntax_trees::Expression::BitFieldRead { .. })
        ));
        assert!(matches!(
            &unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 8, .. })
        ));
    }

    #[test]
    fn recovers_comma_separated_union_variants() {
        let source = r#"
            struct Pair { int value; };
            struct Variants {
                union Named {
                    struct Pair first, second;
                    int scalar;
                } selected;
                int after;
            };
            int read_second(struct Variants* variants) {
                return variants->selected.second.value;
            }
            int read_after(struct Variants* variants) { return variants->after; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, .. })
        ));
        assert!(matches!(
            &unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 4, .. })
        ));
    }

    #[test]
    fn recovers_a_nested_inline_union_pointer_variant() {
        let source = r#"
            struct Command {
                union {
                    union Payload { int value; }* payload;
                    unsigned word;
                };
                int after;
            };
            int read_payload(struct Command* command) {
                return command->payload->value;
            }
            int read_after(struct Command* command) { return command->after; }
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
            Some(mwcc_syntax_trees::Expression::Member { offset: 0, base, .. })
                if matches!(base.as_ref(), mwcc_syntax_trees::Expression::Member { offset: 0, .. })
        ));
        assert!(matches!(
            &unit.functions[1].return_expression,
            Some(mwcc_syntax_trees::Expression::Member { offset: 4, .. })
        ));
    }

    #[test]
    fn accepts_a_trailing_comma_in_scoped_static_arrays() {
        let source = r#"
            void parse(void) {
                if (1) {
                    static unsigned commands[] = {
                        0xED000000,
                        0x005003C0,
                        0xDE010000,
                    };
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
        let commands = unit.functions[0]
            .locals
            .iter()
            .find(|local| local.name == "commands")
            .unwrap();
        assert_eq!(commands.array_length, Some(3));
        assert_eq!(
            commands.data_bytes.as_deref(),
            Some(&[0xED, 0x00, 0x00, 0x00, 0x00, 0x50, 0x03, 0xC0, 0xDE, 0x01, 0x00, 0x00,][..])
        );
    }

    #[test]
    fn serializes_brace_elided_struct_array_elements() {
        let source = r#"
            typedef struct Pair {
                unsigned char offset;
                unsigned char size;
            } Pair;
            typedef struct Asset {
                unsigned id;
                unsigned char characters[3];
                Pair positions[2];
            } Asset;
            Asset assets[2] = {
                1, {'A', 'B'}, {},
                2, {'C'}, {{3, 4}, {5, 6}}
            };
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
            unit.globals[0].data_bytes.as_deref(),
            Some(
                &[
                    0, 0, 0, 1, b'A', b'B', 0, 0, 0, 0, 0, 0, // first Asset
                    0, 0, 0, 2, b'C', 0, 0, 3, 4, 5, 6, 0, // second Asset
                ][..]
            )
        );
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
    fn unsigned_plain_char_keeps_plain_char_cxx_abi_identity() {
        let source = r#"
            unsigned hash(const char* text) { return *text; }
            unsigned explicit_hash(const unsigned char* text) { return *text; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            false,
            1,
            3,
        )
        .unwrap();

        assert_eq!(unit.functions[0].name, "hash__FPCc");
        assert_eq!(unit.functions[1].name, "explicit_hash__FPCUc");
        assert_eq!(
            unit.functions[0].parameters[0].parameter_type,
            mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::UnsignedChar)
        );
    }

    #[test]
    fn long_source_spelling_survives_word_sized_storage_lowering() {
        let source = r#"
            unsigned hash(const char* text, unsigned long size) { return size; }
            long signed_value(long value) { return value; }
        "#;
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            true,
            true,
            1,
            3,
        )
        .unwrap();

        assert_eq!(unit.functions[0].name, "hash__FPCcUl");
        assert_eq!(unit.functions[1].name, "signed_value__Fl");
        assert_eq!(
            unit.functions[0].parameters[1].parameter_type,
            mwcc_syntax_trees::Type::UnsignedInt
        );
        assert_eq!(
            unit.functions[1].parameters[0].parameter_type,
            mwcc_syntax_trees::Type::Int
        );
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
    fn parses_empty_nested_template_type_construction_as_a_value() {
        let source = r#"
            namespace rstl {
                struct optional_object_null {};
                template <typename T>
                struct basic_string { struct literal_t {}; };
                typedef basic_string<int> wstring;
                void construct(void) {
                    wstring::literal_t();
                    rstl::optional_object_null();
                }
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
            [
                mwcc_syntax_trees::Statement::Expression(
                    mwcc_syntax_trees::Expression::AggregateLiteral(first)
                ),
                mwcc_syntax_trees::Statement::Expression(
                    mwcc_syntax_trees::Expression::AggregateLiteral(second)
                ),
            ] if first.is_empty() && second.is_empty()
        ));
    }

    #[test]
    fn lays_out_namespace_qualified_nested_template_instances() {
        let source = r#"
            namespace rstl {
                template <typename T>
                class ownership_transfer {
                    mutable bool owns;
                    mutable T* pointer;
                };
                template <typename T>
                class optional_object {
                    unsigned char data[sizeof(T)];
                    bool valid __attribute__((aligned(4)));
                };
            }
            class Item {};
            rstl::optional_object<rstl::ownership_transfer<Item> > make(void) {}
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
            unit.functions[0].return_type,
            mwcc_syntax_trees::Type::Struct { size: 12, align: 4 }
        );
    }

    #[test]
    fn resolves_injected_nested_class_names_in_out_of_class_members() {
        let source = r#"
            struct Outer {
                struct Inner {
                    virtual bool same(Inner* other);
                    void draw();
                };
            };
            void Outer::Inner::draw() {
                Inner* current = this;
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
            unit.functions[0].locals[0].declared_type,
            mwcc_syntax_trees::Type::StructPointer { .. }
        ));
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

    #[test]
    fn rejects_unlowered_explicit_static_data_specializations() {
        let source = r#"
            template <typename T>
            struct PoolOwner { static T pool; };
            template <> int PoolOwner<int>::pool;
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
            .contains("an explicit C++ template specialization was not lowered"));
    }

    #[test]
    fn retains_volatile_on_file_scope_objects() {
        let source = "static volatile int status; int plain;";
        let unit = parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(unit.globals[0].is_volatile);
        assert!(!unit.globals[1].is_volatile);
    }
}
