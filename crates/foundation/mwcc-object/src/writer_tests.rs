use super::*;
use crate::{
    DebugLayout, DebugRelocation, DebugRelocationKind, DebugSection, DebugSections, DebugSymbol,
    FunctionObject, ObjectFormat,
};

fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn section_index(object: &[u8], name: &str) -> usize {
    let section_headers = be_u32(object, 32) as usize;
    let section_size = be_u16(object, 46) as usize;
    let section_count = be_u16(object, 48) as usize;
    let shstrtab_index = be_u16(object, 50) as usize;
    let shstrtab_header = section_headers + shstrtab_index * section_size;
    let shstrtab_offset = be_u32(object, shstrtab_header + 16) as usize;
    (0..section_count)
        .find(|index| {
            let header = section_headers + index * section_size;
            let name_offset = be_u32(object, header) as usize;
            let start = shstrtab_offset + name_offset;
            let end = object[start..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|length| start + length)
                .unwrap();
            &object[start..end] == name.as_bytes()
        })
        .unwrap_or_else(|| panic!("missing ELF section '{name}'"))
}

fn section_header(object: &[u8], index: usize) -> usize {
    be_u32(object, 32) as usize + index * be_u16(object, 46) as usize
}

fn symbol_names(object: &[u8]) -> Vec<String> {
    let section_headers = be_u32(object, 32) as usize;
    let section_size = be_u16(object, 46) as usize;
    let section_count = be_u16(object, 48) as usize;
    let symtab_index = (0..section_count)
        .find(|index| be_u32(object, section_headers + index * section_size + 4) == SHT_SYMTAB)
        .unwrap();
    let symtab_header = section_headers + symtab_index * section_size;
    let symtab_offset = be_u32(object, symtab_header + 16) as usize;
    let symtab_size = be_u32(object, symtab_header + 20) as usize;
    let strtab_index = be_u32(object, symtab_header + 24) as usize;
    let strtab_header = section_headers + strtab_index * section_size;
    let strtab_offset = be_u32(object, strtab_header + 16) as usize;
    (0..symtab_size / SYMBOL_SIZE)
        .map(|index| {
            let name_offset = be_u32(object, symtab_offset + index * SYMBOL_SIZE) as usize;
            let start = strtab_offset + name_offset;
            let end = object[start..]
                .iter()
                .position(|byte| *byte == 0)
                .map(|length| start + length)
                .unwrap();
            String::from_utf8(object[start..end].to_vec()).unwrap()
        })
        .collect()
}

fn symbol_value_and_size(object: &[u8], wanted: &str) -> (u32, u32) {
    let section_headers = be_u32(object, 32) as usize;
    let section_size = be_u16(object, 46) as usize;
    let section_count = be_u16(object, 48) as usize;
    let symtab_index = (0..section_count)
        .find(|index| be_u32(object, section_headers + index * section_size + 4) == SHT_SYMTAB)
        .unwrap();
    let symtab_header = section_headers + symtab_index * section_size;
    let symtab_offset = be_u32(object, symtab_header + 16) as usize;
    let symtab_size = be_u32(object, symtab_header + 20) as usize;
    let strtab_index = be_u32(object, symtab_header + 24) as usize;
    let strtab_header = section_headers + strtab_index * section_size;
    let strtab_offset = be_u32(object, strtab_header + 16) as usize;
    for index in 0..symtab_size / SYMBOL_SIZE {
        let symbol = symtab_offset + index * SYMBOL_SIZE;
        let name_offset = be_u32(object, symbol) as usize;
        let start = strtab_offset + name_offset;
        let end = object[start..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|length| start + length)
            .unwrap();
        if &object[start..end] == wanted.as_bytes() {
            return (be_u32(object, symbol + 4), be_u32(object, symbol + 8));
        }
    }
    panic!("missing ELF symbol '{wanted}'")
}

fn constant(byte_width: u8, image: bool) -> Sdata2Constant {
    Sdata2Constant {
        bits: 0,
        byte_width,
        static_slot: false,
        image,
        force_new: image,
        force_full_data_section: false,
    }
}

fn weak_function(name: &'static str) -> FunctionObject<'static> {
    FunctionObject {
        name,
        is_static: false,
        static_locals_lead: false,
        text_deferred: false,
        is_weak: true,
        section: None,
        is_asm: false,
        entry_points: Vec::new(),
        force_active: false,
        text: &[0x4e, 0x80, 0x00, 0x20],
        data_section_displacements: Vec::new(),
        relocations: Vec::new(),
        constants: Vec::new(),
        frame: None,
        anonymous_bump: 0,
        implicit_local: false,
        weak_inline: true,
        constant_number_gaps: Vec::new(),
        constant_number_adjust: 0,
        constant_pool_prefix_padding: 0,
        phantom_externals: Vec::new(),
        body_references_precede_symbol: false,
        body_value_references: Vec::new(),
        post_constant_bump: 0,
        post_function_anonymous_bump: None,
        post_function_counter_rollback: 0,
        string_count: 0,
        string_number_after_constants: None,
        string_number_after_rodata: None,
        string_names: Vec::new(),
        jump_tables: Vec::new(),
        jump_table_number_before_constant: None,
        anonymous_rodata: Vec::new(),
        local_undefined_callees: Vec::new(),
        symbol_order: Vec::new(),
        defined_data_precedes_defined_functions: false,
        referenced_function_symbols: Vec::new(),
        implicit_external_callees: Vec::new(),
        early_implicit_external_callees: Vec::new(),
    }
}

#[test]
fn interleaved_jump_tables_resume_at_the_next_pool_ordinal() {
    let tables = [
        crate::JumpTable {
            entries: vec![4],
            anonymous_offset: 1,
        },
        crate::JumpTable {
            entries: vec![8],
            anonymous_offset: 1,
        },
    ];
    let mut next_pool_ordinal = 277;
    assert_eq!(
        assign_interleaved_jump_table_numbers(&mut next_pool_ordinal, &tables),
        [278, 279]
    );
    assert_eq!(next_pool_ordinal, 280);
}

#[test]
fn deferred_body_reference_phases_can_straddle_its_symbol() {
    for order in [
        FunctionSymbolOrder::LegacyDeferred,
        FunctionSymbolOrder::Deferred,
        FunctionSymbolOrder::FunctionFirst,
        FunctionSymbolOrder::ReferencesFirst,
    ] {
        for late_first in [false, true] {
            let mut function = weak_function("owner");
            function.is_weak = false;
            function.weak_inline = false;
            function.body_references_precede_symbol = true;
            if late_first {
                function.body_value_references.push("first".into());
            }
            function.symbol_order = vec!["first".into(), "second".into()];
            function.implicit_external_callees = function.symbol_order.clone();
            function.relocations = function
                .symbol_order
                .iter()
                .cloned()
                .map(|name| crate::TextRelocation {
                    offset: 0,
                    elf_type: 10,
                    target: crate::RelocationTarget::External(name),
                })
                .collect();
            let object = write_object(&ObjectInput {
                source_name: "ordered.cpp",
                object_format: ObjectFormat {
                    comment: CommentFormat {
                        marker: 8,
                        version: (2, 0, 1),
                        pooling_enabled: true,
                        unsigned_char: false,
                    },
                    emb_sda21_offset: 0,
                    code_alignment: 4,
                    sdata2_writable: true,
                    function_symbol_order: order,
                    asm_absolute_references_before_function: false,
                    early_static_functions_after_first_pool: false,
                    bss_anchor_after_first_local_object: false,
                    weak_vtable_function_symbol_tail: false,
                    owned_rtti_closure_relocation_order: false,
                    initialized_globals_before_deferred_functions: true,
                    local_data_symbols_in_declaration_order: false,
                    small_zero_statics_in_declaration_order: false,
                    zero_data_in_declaration_order: false,
                    rodata_anchor_before_data_symbols: false,
                    rodata_anchor_comment_flags: 0,
                    data_relocations_use_section_anchors: false,
                    data_anchor_comment_flags: 0,
                    initial_anonymous_counter: 5,
                    leading_source_anonymous_bump: 0,
                    post_leaf_function_anonymous_bump: 0,
                    post_framed_function_anonymous_bump: 0,
                },
                functions: vec![function],
                data_objects: Vec::new(),
                small_data: true,
                emit_mwcats: false,
                inline_asm_symbols: &[],
                early_static_function_symbols: &[],
                early_undefined_externals: &[],
                section_function_declarations: &[],
                section_externals: &[],
                local_symbol_order: &[],
                debug: None,
            });
            let names = symbol_names(&object);
            let first = names.iter().position(|name| name == "first").unwrap();
            let second = names.iter().position(|name| name == "second").unwrap();
            let owner = names.iter().position(|name| name == "owner").unwrap();

            if order == FunctionSymbolOrder::FunctionFirst {
                assert_eq!((owner + 1, first + 1), (first, second));
            } else if late_first && order != FunctionSymbolOrder::ReferencesFirst {
                assert_eq!((second + 1, owner + 1), (owner, first));
            } else {
                assert_eq!((first + 1, second + 1), (second, owner));
            }
        }
    }
}

#[test]
fn compiler_register_helpers_precede_function_first_body_symbols() {
    let mut function = weak_function("owner");
    function.is_weak = false;
    function.weak_inline = false;
    function.symbol_order = vec![
        "_savegpr_19".into(),
        "memset".into(),
        "_restgpr_19".into(),
    ];
    function.relocations = function
        .symbol_order
        .iter()
        .enumerate()
        .map(|(index, name)| crate::TextRelocation {
            offset: (index * 4) as u32,
            elf_type: 10,
            target: crate::RelocationTarget::External(name.clone()),
        })
        .collect();
    let object = write_object(&ObjectInput {
        source_name: "helpers.c",
        object_format: ObjectFormat {
            comment: CommentFormat {
                marker: 11,
                version: (3, 0, 0),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::FunctionFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![function],
        data_objects: Vec::new(),
        small_data: false,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });
    let names = symbol_names(&object);
    let save = names.iter().position(|name| name == "_savegpr_19").unwrap();
    let restore = names
        .iter()
        .position(|name| name == "_restgpr_19")
        .unwrap();
    let owner = names.iter().position(|name| name == "owner").unwrap();
    let memset = names.iter().position(|name| name == "memset").unwrap();

    assert_eq!((save + 1, restore + 1, owner + 1), (restore, owner, memset));
}

#[test]
fn text_relocations_are_written_in_offset_order() {
    let relocations = vec![
        crate::TextRelocation {
            offset: 14,
            elf_type: 4,
            target: crate::RelocationTarget::External("low".to_owned()),
        },
        crate::TextRelocation {
            offset: 6,
            elf_type: 6,
            target: crate::RelocationTarget::External("high".to_owned()),
        },
        crate::TextRelocation {
            offset: 14,
            elf_type: 5,
            target: crate::RelocationTarget::External("same-offset".to_owned()),
        },
    ];

    assert_eq!(
        text_relocation_order(&relocations)
            .iter()
            .map(|relocation| relocation.offset)
            .collect::<Vec<_>>(),
        [6, 14, 14]
    );
}

#[test]
fn discarded_inline_images_use_aggregate_alignment() {
    assert_eq!(constant_alignment(&constant(8, true)), 4);
    assert_eq!(constant_alignment(&constant(8, false)), 8);
}

#[test]
fn pool_numbers_can_precede_the_ordinary_function_position() {
    assert_eq!(adjusted_pool_number(192, -1), 191);
}

#[test]
fn source_analysis_advances_the_writers_dense_ordinal_stream() {
    assert_eq!(dense_anonymous_counter(2, 43, 0, 0, 0), 45);
    assert_eq!(dense_anonymous_counter(2, 43, 7, 2, 1), 49);
}

#[test]
fn owned_rtti_closures_schedule_base_tables_then_vtable_transactions() {
    let relocation = |offset, target: &str| crate::DataRelocation {
        offset,
        target: target.into(),
        addend: 0,
    };
    let vtable = DataObject {
        name: "__vt__4Boss",
        size: 20,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 20]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: vec![
            relocation(8, "getAge__4BaseFv"),
            relocation(12, "read__4BaseFv"),
            relocation(16, "update__4BaseFv"),
            relocation(0, "__RTTI__4Boss"),
        ],
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: true,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let base_table = DataObject {
        name: "@40",
        size: 12,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 12]),
        is_const: false,
        force_full_data_section: true,
        is_static: true,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: Some(40),
        preassigned_ordinal_advances_counter: true,
        preassigned_pool_prefix_credit: 0,
        relocations: vec![
            relocation(0, "__RTTI__4Base"),
            relocation(8, "__RTTI__4Core"),
        ],
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    // Reverse body-emission order is getAge, read.
    let functions = [
        weak_function("read__4BaseFv"),
        weak_function("getAge__4BaseFv"),
    ];
    let objects = [vtable, base_table];
    let schedule = data_relocation_order(&objects, &functions, &[0, 1], true);
    assert!(schedule.owned_rtti_closure);
    let targets: Vec<&str> = schedule
        .entries
        .iter()
        .map(|&(object, relocation)| objects[object].relocations[relocation].target.as_str())
        .collect();

    assert_eq!(
        targets,
        [
            "__RTTI__4Core",
            "__RTTI__4Base",
            "__RTTI__4Boss",
            "getAge__4BaseFv",
            "read__4BaseFv",
            "update__4BaseFv",
        ]
    );
    assert_eq!(
        owned_vtable_function_symbol_order(&objects, &functions, &[0, 1]),
        [1, 0]
    );
}

#[test]
fn owned_rtti_dispatcher_precedes_template_virtual_leaves_in_slot_order() {
    let relocation = |offset, target: &str| crate::DataRelocation {
        offset,
        target: target.into(),
        addend: 0,
    };
    let mut dispatcher = weak_function("dispatch__12Receiver<1E>Fv");
    dispatcher.text = &[0x60, 0, 0, 0, 0x4e, 0x80, 0, 0x20];
    let functions = [
        weak_function("first__12Receiver<1E>Fv"),
        weak_function("second__12Receiver<1E>Fv"),
        dispatcher,
    ];
    let vtable = DataObject {
        name: "__vt__12Receiver<1E>",
        size: 20,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 20]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: vec![
            relocation(0, "__RTTI__12Receiver<1E>"),
            relocation(8, "dispatch__12Receiver<1E>Fv"),
            relocation(12, "first__12Receiver<1E>Fv"),
            relocation(16, "second__12Receiver<1E>Fv"),
        ],
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: true,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let objects = [vtable];
    let schedule = data_relocation_order(&objects, &functions, &[0], true);
    let targets: Vec<&str> = schedule
        .entries
        .iter()
        .map(|&(object, relocation)| objects[object].relocations[relocation].target.as_str())
        .collect();
    assert_eq!(
        targets,
        [
            "__RTTI__12Receiver<1E>",
            "dispatch__12Receiver<1E>Fv",
            "first__12Receiver<1E>Fv",
            "second__12Receiver<1E>Fv",
        ]
    );
    assert_eq!(
        owned_vtable_function_symbol_order(&objects, &functions, &[0]),
        [2, 0, 1]
    );
}

#[test]
fn owned_rtti_data_layout_interleaves_names_bases_and_vtables() {
    let object = |name: &'static str, relocations: Vec<crate::DataRelocation>| DataObject {
        name,
        size: 12,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 12]),
        is_const: false,
        force_full_data_section: true,
        is_static: name.starts_with('@') || name.starts_with("__RTTI__"),
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: name.starts_with('@').then_some(40),
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations,
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let relocation = |offset: u32, target: &str| crate::DataRelocation {
        offset,
        target: target.into(),
        addend: 0,
    };
    let objects = vec![
        object("@leaf-name", Vec::new()),
        object("__vt__4Boss", vec![relocation(0, "__RTTI__4Boss")]),
        object("@boss-bases", vec![relocation(0, "__RTTI__4Base")]),
        object("@boss-name", Vec::new()),
        object(
            "__RTTI__4Base",
            vec![relocation(0, "@base-name"), relocation(4, "@base-bases")],
        ),
        object(
            "__RTTI__4Boss",
            vec![
                relocation(0, "@boss-name"),
                relocation(4, "@boss-bases"),
            ],
        ),
        object("@base-name", Vec::new()),
        object("@base-bases", vec![relocation(0, "__RTTI__4Leaf")]),
        object("__RTTI__4Leaf", vec![relocation(0, "@leaf-name")]),
    ];

    assert_eq!(
        owned_rtti_data_layout_order(&objects),
        [0, 6, 7, 3, 2, 1],
        "base closure from leaf to root, then vtable"
    );
    assert_eq!(
        owned_rtti_local_symbol_order(&objects),
        [3, 6, 0, 8, 7, 4, 2, 5],
        "root name, recursive base transactions, root bases and handle"
    );
}

#[test]
fn owned_rtti_frontier_follows_the_late_weak_body_group() {
    let mut ordinary = weak_function("ordinary");
    ordinary.weak_inline = false;
    let weak = weak_function("weak");
    let object = DataObject {
        name: "@rtti-name",
        size: 4,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 4]),
        is_const: false,
        force_full_data_section: true,
        is_static: true,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };

    assert_eq!(owned_rtti_source_frontier(&[object], &[ordinary, weak], &[0]), 1);
}

#[test]
fn pooled_root_rtti_layout_keeps_the_constructor_string_in_body_order() {
    let object = |name: &'static str, relocations: Vec<crate::DataRelocation>| DataObject {
        name,
        size: 12,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 12]),
        is_const: false,
        force_full_data_section: true,
        is_static: name.starts_with('@') || name.starts_with("__RTTI__"),
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations,
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let relocation = |offset: u32, target: &str| crate::DataRelocation {
        offset,
        target: target.into(),
        addend: 0,
    };
    let mut first_name = object("@390", Vec::new());
    first_name.preassigned_anonymous_ordinal = Some(390);
    let mut second_name = object("@391", Vec::new());
    second_name.preassigned_anonymous_ordinal = Some(391);
    let objects = vec![
        object("@388", Vec::new()),
        first_name,
        second_name,
        object("@392", vec![relocation(0, "__RTTI__4Leaf")]),
        object("@unused", Vec::new()),
        object(
            "@389",
            vec![
                relocation(0, "__RTTI__5First"),
                relocation(8, "__RTTI__6Second"),
            ],
        ),
        object("__RTTI__5First", vec![relocation(0, "@390")]),
        object(
            "__RTTI__6Second",
            vec![relocation(0, "@391"), relocation(4, "@392")],
        ),
        object(
            "__RTTI__4Root",
            vec![relocation(0, "@388"), relocation(4, "@389")],
        ),
        object("__vt__5First", vec![relocation(0, "__RTTI__5First")]),
        object("__vt__6Second", vec![relocation(0, "__RTTI__6Second")]),
        object("__vt__4Root", vec![relocation(0, "__RTTI__4Root")]),
    ];

    assert_eq!(
        owned_rtti_data_layout_order(&objects),
        [1, 2, 3, 5, 11, 10, 9],
    );
}

#[test]
fn data_relocations_follow_interleaved_creation_order() {
    let descriptor = DataObject {
        name: "descriptor",
        size: 8,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 8]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: vec![
            crate::DataRelocation {
                offset: 0,
                target: "first".into(),
                addend: 0,
            },
            crate::DataRelocation {
                offset: 4,
                target: "second".into(),
                addend: 0,
            },
        ],
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let mut function = weak_function("dispatch");
    function.is_weak = false;
    function.weak_inline = false;
    function.jump_tables.push(crate::JumpTable {
        entries: vec![4, 8],
        anonymous_offset: 0,
    });
    let object = write_object(&ObjectInput {
        source_name: "mixed.c",
        object_format: ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 3, 3),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![function],
        data_objects: vec![descriptor],
        small_data: false,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });

    let rela_data = section_index(&object, ".rela.data");
    let header = section_header(&object, rela_data);
    let offset = be_u32(&object, header + 16) as usize;
    let size = be_u32(&object, header + 20) as usize;
    let relocation_offsets: Vec<u32> = (0..size / 12)
        .map(|index| be_u32(&object, offset + index * 12))
        .collect();
    assert_eq!(relocation_offsets, [4, 0, 8, 12]);
}

#[test]
fn analysis_constant_placement_is_independent_of_counter_advancement() {
    let source_constant = DataObject {
        name: "header_constant",
        size: 8,
        alignment: 8,
        comment_alignment: 8,
        initial_bytes: Some(vec![0xcc; 8]),
        is_const: true,
        force_full_data_section: false,
        is_static: true,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    let trailing = DataObject {
        name: "@190",
        size: 2,
        alignment: 2,
        comment_alignment: 2,
        initial_bytes: Some(vec![0xaa, 0xbb]),
        is_const: true,
        force_full_data_section: false,
        is_static: true,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: Some(190),
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    assert!(is_trailing_analysis_constant(&trailing));
    let residue = DataObject {
        preassigned_pool_prefix_credit: 4,
        ..trailing
    };
    assert!(!is_trailing_analysis_constant(&residue));
    assert!(is_interstitial_analysis_constant(&residue));
    let text = [0x4e, 0x80, 0x00, 0x20];
    let function = FunctionObject {
        name: "f",
        is_static: false,
        static_locals_lead: false,
        text_deferred: false,
        is_weak: false,
        section: None,
        is_asm: false,
        entry_points: Vec::new(),
        force_active: false,
        text: &text,
        data_section_displacements: Vec::new(),
        relocations: Vec::new(),
        constants: vec![Sdata2Constant {
            bits: 0x1122_3344_5566_7788,
            byte_width: 8,
            static_slot: false,
            image: false,
            force_new: false,
            force_full_data_section: false,
        }],
        frame: None,
        anonymous_bump: 0,
        implicit_local: false,
        weak_inline: false,
        constant_number_gaps: Vec::new(),
        constant_number_adjust: 0,
        constant_pool_prefix_padding: 4,
        phantom_externals: Vec::new(),
        body_references_precede_symbol: false,
        body_value_references: Vec::new(),
        post_constant_bump: 0,
        post_function_anonymous_bump: None,
        post_function_counter_rollback: 0,
        string_count: 0,
        string_number_after_constants: None,
        string_number_after_rodata: None,
        string_names: Vec::new(),
        jump_tables: Vec::new(),
        jump_table_number_before_constant: None,
        anonymous_rodata: Vec::new(),
        local_undefined_callees: Vec::new(),
        symbol_order: Vec::new(),
        defined_data_precedes_defined_functions: false,
        referenced_function_symbols: Vec::new(),
        implicit_external_callees: Vec::new(),
        early_implicit_external_callees: Vec::new(),
    };
    let object = write_object(&ObjectInput {
        source_name: "residue.cpp",
        object_format: ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 4, 7),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![function],
        data_objects: vec![source_constant, residue],
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });

    let sdata2 = section_index(&object, ".sdata2");
    let header = section_header(&object, sdata2);
    let offset = be_u32(&object, header + 16) as usize;
    let size = be_u32(&object, header + 20) as usize;
    assert_eq!(
        &object[offset..offset + size],
        &[
            0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xaa, 0xbb, 0, 0, 0, 0, 0, 0,
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        ]
    );
    assert_eq!(symbol_value_and_size(&object, "@190"), (8, 2));
    assert_eq!(symbol_value_and_size(&object, "@1"), (16, 8));
}

#[test]
fn function_pool_prefix_padding_precedes_alignment_of_its_first_fresh_slot() {
    let mut first = weak_function("first");
    first.constants = vec![Sdata2Constant {
        bits: 0x1111_1111,
        byte_width: 4,
        static_slot: false,
        image: false,
        force_new: false,
        force_full_data_section: false,
    }];
    let mut padded = weak_function("padded");
    padded.constant_pool_prefix_padding = 4;
    padded.constants = vec![
        Sdata2Constant {
            bits: 0x2222_2222,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
            force_full_data_section: false,
        },
        Sdata2Constant {
            bits: 0x3333_3333,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
            force_full_data_section: false,
        },
        Sdata2Constant {
            bits: 0x4444_4444,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
            force_full_data_section: false,
        },
        Sdata2Constant {
            bits: 0x5555_5555_6666_6666,
            byte_width: 8,
            static_slot: false,
            image: false,
            force_new: false,
            force_full_data_section: false,
        },
    ];
    let object = write_object(&ObjectInput {
        source_name: "pool-padding.cpp",
        object_format: ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 4, 7),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![first, padded],
        data_objects: Vec::new(),
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });

    let sdata2 = section_index(&object, ".sdata2");
    let header = section_header(&object, sdata2);
    let offset = be_u32(&object, header + 16) as usize;
    let size = be_u32(&object, header + 20) as usize;
    assert_eq!(
        &object[offset..offset + size],
        &[
            0x11, 0x11, 0x11, 0x11, 0, 0, 0, 0, 0x22, 0x22, 0x22, 0x22, 0x33, 0x33,
            0x33, 0x33, 0x44, 0x44, 0x44, 0x44, 0, 0, 0, 0, 0x55, 0x55, 0x55, 0x55,
            0x66, 0x66, 0x66, 0x66,
        ]
    );
    assert_eq!(symbol_value_and_size(&object, "@1"), (0, 4));
    assert_eq!(symbol_value_and_size(&object, "@2"), (8, 4));
    assert_eq!(symbol_value_and_size(&object, "@3"), (12, 4));
    assert_eq!(symbol_value_and_size(&object, "@4"), (16, 4));
    assert_eq!(symbol_value_and_size(&object, "@5"), (24, 8));
}

#[test]
fn data_section_displacements_patch_only_the_d_form_immediate() {
    let mut text = vec![0xa0, 0x63, 0, 0];
    let sections = HashMap::from([("table", ".data")]);
    let offsets = HashMap::from([("table", 0x1c)]);
    apply_data_section_displacements(
        &mut text,
        &[(2, DataSectionDisplacementTarget::Symbol("table".to_owned()))],
        &sections,
        &offsets,
        &[],
    );
    assert_eq!(text, [0xa0, 0x63, 0, 0x1c]);
}

#[test]
fn bss_section_displacements_add_to_selected_member_offsets() {
    let mut text = vec![0x90, 0x85, 0, 12];
    let sections = HashMap::from([("state", ".bss")]);
    let offsets = HashMap::from([("state", 0x10)]);
    apply_data_section_displacements(
        &mut text,
        &[(2, DataSectionDisplacementTarget::Symbol("state".to_owned()))],
        &sections,
        &offsets,
        &[],
    );
    assert_eq!(text, [0x90, 0x85, 0, 0x1c]);
}

#[test]
fn bss_section_displacements_retain_the_selected_page_low_half() {
    let mut text = vec![0x80, 0x05, 0, 0];
    let sections = HashMap::from([("ring", ".bss")]);
    let offsets = HashMap::from([("ring", 0x1_4080)]);
    apply_data_section_displacements(
        &mut text,
        &[(2, DataSectionDisplacementTarget::Symbol("ring".to_owned()))],
        &sections,
        &offsets,
        &[],
    );
    assert_eq!(text, [0x80, 0x05, 0x40, 0x80]);
}

#[test]
fn anonymous_rodata_displacements_add_the_final_blob_offset() {
    let mut text = vec![0x80, 0xa3, 0, 4];
    apply_data_section_displacements(
        &mut text,
        &[(2, DataSectionDisplacementTarget::AnonymousRodata(1))],
        &HashMap::new(),
        &HashMap::new(),
        &[0x20, 0x30],
    );
    assert_eq!(text, [0x80, 0xa3, 0, 0x34]);
}

#[test]
fn comment_header_records_pooling_mode() {
    let enabled = comment_record(
        CommentFormat {
            marker: 0x08,
            version: (2, 3, 0),
            pooling_enabled: true,
            unsigned_char: false,
        },
        &[],
    );
    let disabled = comment_record(
        CommentFormat {
            marker: 0x08,
            version: (2, 3, 0),
            pooling_enabled: false,
            unsigned_char: false,
        },
        &[],
    );
    assert_eq!(enabled[11], 0x08);
    assert_eq!(&enabled[12..16], &[2, 3, 0, 1]);
    assert_eq!(enabled[16], 1);
    assert_eq!(disabled[16], 0);
}

#[test]
fn only_modern_comment_headers_record_unsigned_character_mode() {
    for (marker, version, unsigned_byte) in [
        (0x08, (2, 3, 0), 0),
        (0x0a, (2, 4, 2), 0),
        (0x0b, (2, 4, 7), 0),
        (0x0e, (4, 0, 0), 1),
        (0x0f, (4, 0, 0), 1),
    ] {
        let format = CommentFormat {
            marker,
            version,
            pooling_enabled: true,
            unsigned_char: false,
        };
        let signed = comment_record(format, &[]);
        let unsigned = comment_record(CommentFormat { unsigned_char: true, ..format }, &[]);
        assert_eq!(signed[22], 0);
        assert_eq!(unsigned[22], unsigned_byte);
        assert_eq!(&signed[..22], &unsigned[..22]);
        assert_eq!(&signed[23..], &unsigned[23..]);
    }
}

#[test]
fn leading_pure_vtable_slot_defers_defined_function_symbols() {
    let vtable = DataObject {
        name: "__vt__8Abstract",
        size: 16,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 16]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: vec![crate::DataRelocation {
            offset: 12,
            target: "read__8AbstractFv".into(),
            addend: 0,
        }],
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    assert!(defers_defined_vtable_function_targets(&vtable));

    let concrete = DataObject {
        relocations: vec![crate::DataRelocation {
            offset: 8,
            target: "read__8ConcreteFv".into(),
            addend: 0,
        }],
        ..vtable
    };
    assert!(!defers_defined_vtable_function_targets(&concrete));
    assert_eq!(data_comment_flags(&concrete), 0);

    let retained = DataObject {
        force_active: true,
        ..concrete
    };
    assert_eq!(data_comment_flags(&retained), FORCE_ACTIVE_FLAG);
}

#[test]
fn deferred_weak_vtable_waits_for_its_function_reference() {
    let vtable = DataObject {
        name: "__vt__8Inline",
        size: 12,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![0; 12]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: true,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    assert!(!initialized_object_is_upfront(&vtable, true));

    let ordinary = DataObject {
        name: "ordinary",
        is_weak: false,
        ..vtable
    };
    assert!(initialized_object_is_upfront(&ordinary, true));
}

#[test]
fn build_163_deferred_large_zero_object_is_registered_upfront() {
    let object = DataObject {
        name: "records",
        size: 0x19b0,
        alignment: 8,
        comment_alignment: 8,
        initial_bytes: None,
        is_const: false,
        force_full_data_section: false,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
    assert!(deferred_large_zero_object_is_upfront(
        &object,
        FunctionSymbolOrder::LegacyDeferred,
        true,
        ".bss"
    ));
    assert!(deferred_large_zero_object_is_upfront(
        &object,
        FunctionSymbolOrder::Deferred,
        true,
        ".bss"
    ));
    assert!(!deferred_large_zero_object_is_upfront(
        &object,
        FunctionSymbolOrder::LegacyDeferred,
        true,
        ".sbss"
    ));
}

#[test]
fn grouped_debug_data_relocations_restore_source_declaration_order() {
    let data = [
        DataObject {
            name: "__vt__8Inline",
            size: 12,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![0; 12]),
            is_const: false,
            force_full_data_section: true,
            is_static: false,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 1,
            functions_before: 1,
            is_weak: true,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
        DataObject {
            name: "instance",
            size: 4,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![0; 4]),
            is_const: false,
            force_full_data_section: false,
            is_static: false,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: vec![crate::DataRelocation {
                offset: 0,
                target: "__vt__8Inline".into(),
                addend: 0,
            }],
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
    ];
    let debug = DebugSections {
        layout: DebugLayout::BetweenFullAndSmallDataGrouped,
        post_framed_function_anonymous_bump_override: None,
        line: Vec::new(),
        debug: vec![0; 8],
        line_relocations: Vec::new(),
        debug_relocations: vec![
            DebugRelocation {
                offset: 0,
                kind: DebugRelocationKind::UnalignedAddress32,
                target: DebugRelocationTarget::Symbol("instance".into()),
                addend: 0,
            },
            DebugRelocation {
                offset: 4,
                kind: DebugRelocationKind::UnalignedAddress32,
                target: DebugRelocationTarget::Symbol("__vt__8Inline".into()),
                addend: 0,
            },
        ],
        symbols: vec![DebugSymbol {
            name: ".dwarf.0006.constructor".into(),
            section: DebugSection::Debug,
            offset: 0,
            size: 0,
            alignment: 1,
            comment_flags: 0,
            binding: DebugSymbolBinding::Local,
            placement: DebugSymbolPlacement::Early,
        }],
        captured_local_symbol_order: Vec::new(),
    };
    let object = write_object(&ObjectInput {
        source_name: "class.cpp",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 4, 7),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::Deferred,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: Vec::new(),
        data_objects: data.into(),
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: Some(debug),
    });
    let names = symbol_names(&object);
    let instance = names.iter().position(|name| name == "instance").unwrap();
    let vtable = names
        .iter()
        .position(|name| name == "__vt__8Inline")
        .unwrap();
    assert_eq!(instance + 1, vtable);
    assert!(
        section_index(&object, ".rela.debug") < section_index(&object, ".rela.sdata"),
        "between-data debug relocations precede small-data relocations"
    );
}

#[test]
fn data_anchor_precedes_the_first_upfront_local_data_object() {
    let data = [
        DataObject {
            name: "small",
            size: 4,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![1; 4]),
            is_const: false,
            force_full_data_section: false,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
        DataObject {
            name: "full",
            size: 12,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![2; 12]),
            is_const: false,
            force_full_data_section: false,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
        DataObject {
            name: "pointer",
            size: 4,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![0; 4]),
            is_const: false,
            force_full_data_section: false,
            is_static: false,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: vec![crate::DataRelocation {
                offset: 0,
                target: "full".into(),
                addend: 0,
            }],
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
    ];
    let object = write_object(&ObjectInput {
        source_name: "data.c",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 3, 0),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: true,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: Vec::new(),
        data_objects: data.into(),
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });
    let names = symbol_names(&object);
    let small = names.iter().position(|name| name == "small").unwrap();
    let anchor = names.iter().position(|name| name == "...data.0").unwrap();
    let full = names.iter().position(|name| name == "full").unwrap();
    assert_eq!((small + 1, anchor + 1), (anchor, full));
}

#[test]
fn code_data_anchor_precedes_pools_when_full_data_is_declared_upfront() {
    let mut function = weak_function("probe");
    function.is_weak = false;
    function.weak_inline = false;
    function.relocations = vec![crate::TextRelocation {
        offset: 2,
        elf_type: 6,
        target: crate::RelocationTarget::External("...data.0".into()),
    }];
    function.constants = vec![Sdata2Constant {
        bits: 0x3f80_0000,
        byte_width: 4,
        static_slot: false,
        image: false,
        force_new: false,
        force_full_data_section: false,
    }];

    let data = [DataObject {
        name: "table",
        size: 12,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes: Some(vec![1; 12]),
        is_const: false,
        force_full_data_section: true,
        is_static: false,
        force_active: false,
        is_explicit_zero: false,
        preassigned_anonymous_ordinal: None,
        preassigned_ordinal_advances_counter: false,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    }];
    let object = write_object(&ObjectInput {
        source_name: "anchor.c",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (1, 2, 5),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![function],
        data_objects: data.into(),
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });

    let names = symbol_names(&object);
    let anchor = names.iter().position(|name| name == "...data.0").unwrap();
    let pool = names.iter().position(|name| name.starts_with('@')).unwrap();
    let table = names.iter().position(|name| name == "table").unwrap();
    assert!(anchor < pool);
    assert!(pool < table);
}

#[test]
fn code_data_anchor_follows_earlier_static_functions_before_an_owned_string() {
    let mut first = weak_function("first");
    first.is_static = true;
    first.is_weak = false;
    first.weak_inline = false;
    let mut second = weak_function("second");
    second.is_static = true;
    second.is_weak = false;
    second.weak_inline = false;
    let mut owner = weak_function("owner");
    owner.is_weak = false;
    owner.weak_inline = false;
    owner.string_count = 1;
    owner.string_names = vec!["@1".into()];
    owner.relocations = vec![crate::TextRelocation {
        offset: 2,
        elf_type: 6,
        target: crate::RelocationTarget::External("...data.0".into()),
    }];

    let object = write_object(&ObjectInput {
        source_name: "anchor.c",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (1, 2, 5),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![first, second, owner],
        data_objects: vec![DataObject {
            name: "@1",
            size: 4,
            alignment: 1,
            comment_alignment: 1,
            initial_bytes: Some(vec![0; 4]),
            is_const: false,
            force_full_data_section: true,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: Some(1),
            preassigned_ordinal_advances_counter: true,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        }],
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });
    let names = symbol_names(&object);
    let first = names.iter().position(|name| name == "first").unwrap();
    let second = names.iter().position(|name| name == "second").unwrap();
    let anchor = names.iter().position(|name| name == "...data.0").unwrap();
    let string = names.iter().position(|name| name == "@1").unwrap();

    assert_eq!((first + 1, second + 1, anchor + 1), (second, anchor, string));
}

#[test]
fn const_pointer_arrays_emit_reverse_rodata_relocations() {
    let data = [
        DataObject {
            name: "strings",
            size: 12,
            alignment: 4,
            comment_alignment: 1,
            initial_bytes: Some(vec![1; 12]),
            is_const: false,
            force_full_data_section: true,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
        DataObject {
            name: "table",
            size: 12,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(vec![0; 12]),
            is_const: true,
            force_full_data_section: true,
            is_static: false,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: vec![
                crate::DataRelocation {
                    offset: 0,
                    target: "strings".into(),
                    addend: 0,
                },
                crate::DataRelocation {
                    offset: 4,
                    target: "strings".into(),
                    addend: 4,
                },
                crate::DataRelocation {
                    offset: 8,
                    target: "strings".into(),
                    addend: 8,
                },
            ],
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        },
    ];
    let object = write_object(&ObjectInput {
        source_name: "table.c",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 3, 0),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: true,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: Vec::new(),
        data_objects: data.into(),
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    });

    let rodata = section_index(&object, ".rodata");
    let rela_rodata = section_index(&object, ".rela.rodata");
    let symtab = section_index(&object, ".symtab");
    let rela_header = section_header(&object, rela_rodata);
    assert_eq!(be_u32(&object, rela_header + 24) as usize, symtab);
    assert_eq!(be_u32(&object, rela_header + 28) as usize, rodata);
    assert_eq!(be_u32(&object, rela_header + 36), 12);

    let anchor = symbol_names(&object)
        .iter()
        .position(|name| name == "...data.0")
        .unwrap() as u32;
    let offset = be_u32(&object, rela_header + 16) as usize;
    let size = be_u32(&object, rela_header + 20) as usize;
    assert_eq!(size, 36);
    let records: Vec<_> = (0..size / 12)
        .map(|index| {
            let entry = offset + index * 12;
            let info = be_u32(&object, entry + 4);
            (
                be_u32(&object, entry),
                info >> 8,
                info & 0xff,
                be_u32(&object, entry + 8),
            )
        })
        .collect();
    assert_eq!(
        records,
        [
            (8, anchor, R_PPC_ADDR32, 8),
            (4, anchor, R_PPC_ADDR32, 4),
            (0, anchor, R_PPC_ADDR32, 0),
        ]
    );
}

fn static_frontier_input(position: usize, referenced: bool) -> ObjectInput<'static> {
    let mut before = weak_function("before");
    before.is_static = true;
    before.is_weak = false;
    before.weak_inline = false;
    let mut after = weak_function("after");
    after.is_static = true;
    after.is_weak = false;
    after.weak_inline = false;
    if referenced {
        after.text = &[0x80, 0x60, 0, 0, 0x80, 0x80, 0, 0, 0x4e, 0x80, 0, 0x20];
        after.relocations = ["second", "first"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| crate::TextRelocation {
                offset: 4 * index as u32,
                elf_type: 109,
                target: RelocationTarget::External(name.into()),
            })
            .collect();
    }
    let objects = ["first", "second"]
        .into_iter()
        .map(|name| DataObject {
            name,
            size: 4,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: None,
            is_const: false,
            force_full_data_section: false,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            preassigned_pool_prefix_credit: 0,
            relocations: Vec::new(),
            non_static_functions_before: 0,
            functions_before: position,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        })
        .collect();
    ObjectInput {
        source_name: "frontiers.c",
        object_format: ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 0, 1),
                pooling_enabled: true,
                unsigned_char: false,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: true,
            function_symbol_order: FunctionSymbolOrder::LegacyDeferred,
            asm_absolute_references_before_function: false,
            early_static_functions_after_first_pool: false,
            bss_anchor_after_first_local_object: false,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: true,
            local_data_symbols_in_declaration_order: true,
            small_zero_statics_in_declaration_order: false,
            zero_data_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: false,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 5,
            leading_source_anonymous_bump: 0,
            post_leaf_function_anonymous_bump: 0,
            post_framed_function_anonymous_bump: 0,
        },
        functions: vec![before, after],
        data_objects: objects,
        small_data: true,
        emit_mwcats: false,
        inline_asm_symbols: &[],
        early_static_function_symbols: &[],
        early_undefined_externals: &[],
        section_function_declarations: &[],
        section_externals: &[],
        local_symbol_order: &[],
        debug: None,
    }
}

#[test]
fn declaration_order_statics_keep_front_middle_and_tail_events() {
    for referenced in [false, true] {
        for (position, expected) in [
            (0, ["first", "second", "before", "after"]),
            (1, ["before", "first", "second", "after"]),
            (2, ["before", "after", "first", "second"]),
            (7, ["before", "after", "first", "second"]),
        ] {
            let bytes = write_object(&static_frontier_input(position, referenced));
            if referenced {
                let header = section_header(&bytes, section_index(&bytes, ".rela.text"));
                let offset = be_u32(&bytes, header + 16) as usize;
                let all_names = symbol_names(&bytes);
                for (index, expected) in ["second", "first"].into_iter().enumerate() {
                    let info = be_u32(&bytes, offset + index * 12 + 4);
                    assert_eq!(info & 255, 109);
                    assert_eq!(all_names[(info >> 8) as usize], expected);
                }
            }
            let names: Vec<_> = symbol_names(&bytes)
                .into_iter()
                .filter(|name| ["before", "after", "first", "second"].contains(&name.as_str()))
                .collect();
            assert_eq!(
                names, expected,
                "position {position}, referenced {referenced}"
            );
        }
    }
}

#[test]
fn declaration_order_data_only_statics_keep_local_initializer_bindings() {
    let mut input = static_frontier_input(0, false);
    input.functions.clear();
    input.object_format.zero_data_in_declaration_order = true;
    input.data_objects[1].is_explicit_zero = true;
    let mut pointer = static_frontier_input(0, false).data_objects.remove(0);
    pointer.name = "pointer";
    pointer.is_static = false;
    pointer.initial_bytes = Some(vec![0; 4]);
    pointer.relocations.push(crate::DataRelocation {
        offset: 0,
        target: "first".into(),
        addend: 0,
    });
    input.data_objects.push(pointer);
    let bytes = write_object(&input);
    let names = symbol_names(&bytes);
    let first = names.iter().position(|name| name == "first").unwrap();
    assert_eq!(&names[first..], ["first", "second", "pointer"]);
    let header = section_header(&bytes, section_index(&bytes, ".symtab"));
    let offset = be_u32(&bytes, header + 16) as usize;
    for index in [first, first + 1] {
        let symbol = offset + index * SYMBOL_SIZE;
        assert_eq!(bytes[symbol + 12], STB_LOCAL_OBJECT);
        assert_eq!(
            be_u16(&bytes, symbol + 14) as usize,
            section_index(&bytes, ".sbss")
        );
    }
    let header = section_header(&bytes, section_index(&bytes, ".rela.sdata"));
    let offset = be_u32(&bytes, header + 16) as usize;
    assert_eq!(be_u32(&bytes, offset + 4) >> 8, first as u32);
}

#[test]
fn bss_initializer_anchors_follow_declarations_and_include_object_offsets() {
    for position in [0, 1, 2] {
        let mut input = static_frontier_input(position, false);
        input.object_format.zero_data_in_declaration_order = true;
        input.object_format.data_relocations_use_section_anchors = true;
        input.object_format.data_anchor_comment_flags = 0x0010_0000;
        input.data_objects[0].size = 12;
        input.data_objects[1].size = 20;
        let mut pointer = static_frontier_input(position, false).data_objects.remove(0);
        pointer.name = "pointer";
        pointer.is_static = false;
        pointer.initial_bytes = Some(vec![0; 4]);
        pointer.relocations.push(crate::DataRelocation {
            offset: 0,
            target: "second".into(),
            addend: 3,
        });
        input.data_objects.push(pointer);
        let bytes = write_object(&input);
        let names = symbol_names(&bytes);
        let first = names.iter().position(|name| name == "first").unwrap();
        assert_eq!(&names[first..first + 3], ["first", "...bss.0", "second"]);
        let anchor = first + 1;
        let before = names.iter().position(|name| name == "before").unwrap();
        let after = names.iter().position(|name| name == "after").unwrap();
        assert_eq!(anchor > before, position > 0);
        assert_eq!(anchor > after, position > 1);
        let header = section_header(&bytes, section_index(&bytes, ".rela.sdata"));
        let offset = be_u32(&bytes, header + 16) as usize;
        assert_eq!(be_u32(&bytes, offset + 4) >> 8, anchor as u32);
        assert_eq!(be_u32(&bytes, offset + 8), 15);
        let header = section_header(&bytes, section_index(&bytes, ".comment"));
        let offset = be_u32(&bytes, header + 16) as usize;
        let size = be_u32(&bytes, header + 20) as usize;
        let record = offset + size - names.len() * 8 + anchor * 8;
        assert_eq!(be_u32(&bytes, record), 1);
        assert_eq!(be_u32(&bytes, record + 4), 0x0010_0000);
    }
}

#[test]
fn bss_initializer_anchor_eligibility_distinguishes_c_tentative_globals() {
    for cxx in [false, true] {
        let mut input = static_frontier_input(0, false);
        input.functions.clear();
        input.object_format.zero_data_in_declaration_order = cxx;
        input.object_format.data_relocations_use_section_anchors = true;
        for object in &mut input.data_objects {
            object.is_static = false;
            object.size = 12;
        }
        let mut pointer = static_frontier_input(0, false).data_objects.remove(0);
        pointer.name = "pointer";
        pointer.is_static = false;
        pointer.initial_bytes = Some(vec![0; 4]);
        pointer.relocations.push(crate::DataRelocation {
            offset: 0,
            target: "first".into(),
            addend: 3,
        });
        input.data_objects.push(pointer);
        let bytes = write_object(&input);
        let names = symbol_names(&bytes);
        assert_eq!(names.iter().any(|name| name == "...bss.0"), cxx);
        let header = section_header(&bytes, section_index(&bytes, ".rela.sdata"));
        let offset = be_u32(&bytes, header + 16) as usize;
        let symbol = (be_u32(&bytes, offset + 4) >> 8) as usize;
        assert_eq!(names[symbol], if cxx { "...bss.0" } else { "first" });
        if cxx {
            assert!(symbol < names.iter().position(|name| name == "pointer").unwrap());
        }
        let section_addend = if cxx {
            symbol_value_and_size(&bytes, "first").0
        } else {
            0
        };
        assert_eq!(be_u32(&bytes, offset + 8), section_addend + 3);
    }
}

#[test]
fn cxx_zero_storage_interleaves_exported_definitions_and_body_local_statics() {
    for full in [false, true] {
        let mut input = static_frontier_input(0, false);
        input.object_format.zero_data_in_declaration_order = true;
        input.object_format.function_symbol_order = FunctionSymbolOrder::ReferencesFirst;
        input
            .object_format
            .initialized_globals_before_deferred_functions = false;
        for function in &mut input.functions {
            function.is_static = false;
        }
        for object in &mut input.data_objects {
            object.is_static = false;
            object.size = if full { 12 } else { 4 };
        }
        input.data_objects[1].functions_before = 1;
        let mut local = static_frontier_input(0, false).data_objects.remove(0);
        local.name = "body_local";
        local.static_local_owner = Some(0);
        local.force_full_data_section = full;
        input.data_objects.push(local);
        let bytes = write_object(&input);
        let names = symbol_names(&bytes);
        let local = names
            .iter()
            .find(|name| name.starts_with("body_local$"))
            .unwrap();
        let width = if full { 12 } else { 4 };
        assert_eq!(symbol_value_and_size(&bytes, "first"), (0, width));
        assert_eq!(symbol_value_and_size(&bytes, local), (width, 4));
        assert_eq!(symbol_value_and_size(&bytes, "second"), (width + 4, width));
        let globals: Vec<_> = names
            .iter()
            .filter(|name| ["first", "before", "second", "after"].contains(&name.as_str()))
            .map(String::as_str)
            .collect();
        assert_eq!(globals, ["first", "before", "second", "after"]);
    }
}

#[test]
fn bss_initializer_before_definition_keeps_named_target_after_later_anchor_use() {
    let mut input = static_frontier_input(0, false);
    input.functions.clear();
    input.object_format.zero_data_in_declaration_order = true;
    input.object_format.data_relocations_use_section_anchors = true;
    let mut target = input.data_objects.remove(0);
    target.name = "target";
    target.is_static = false;
    target.size = 12;
    input.data_objects.clear();
    for name in ["before_pointer", "after_pointer"] {
        let mut pointer = static_frontier_input(0, false).data_objects.remove(0);
        pointer.name = name;
        pointer.is_static = false;
        pointer.initial_bytes = Some(vec![0; 4]);
        pointer.relocations.push(crate::DataRelocation {
            offset: 0,
            target: "target".into(),
            addend: 0,
        });
        input.data_objects.push(pointer);
    }
    input.data_objects.insert(1, target);
    let bytes = write_object(&input);
    let names = symbol_names(&bytes);
    let header = section_header(&bytes, section_index(&bytes, ".rela.sdata"));
    let offset = be_u32(&bytes, header + 16) as usize;
    let targets: Vec<_> = (0..2)
        .map(|index| {
            let symbol = be_u32(&bytes, offset + index * 12 + 4) >> 8;
            names[symbol as usize].as_str()
        })
        .collect();
    assert_eq!(targets, ["target", "...bss.0"]);
}

#[test]
fn tentative_statics_follow_the_discovering_function_and_unused_tail() {
    for (order, full, expected) in [
        (
            FunctionSymbolOrder::ReferencesFirst,
            false,
            vec![
                "before", "second", "first", "after", "end", "unused_b", "unused_a",
            ],
        ),
        (
            FunctionSymbolOrder::FunctionFirst,
            false,
            vec![
                "before", "after", "second", "first", "end", "unused_b", "unused_a",
            ],
        ),
        (
            FunctionSymbolOrder::FunctionFirst,
            true,
            vec![
                "before", "first", "after", "second", "end", "unused_b", "unused_a",
            ],
        ),
    ] {
        let mut input = static_frontier_input(0, true);
        input.object_format.local_data_symbols_in_declaration_order = false;
        input.object_format.function_symbol_order = order;
        if full {
            input.data_objects[0].size = 64;
        }
        let mut end = weak_function("end");
        end.is_static = true;
        end.is_weak = false;
        end.weak_inline = false;
        input.functions.push(end);
        for name in ["unused_a", "unused_b"] {
            let mut object = static_frontier_input(0, false).data_objects.remove(1);
            object.name = name;
            input.data_objects.push(object);
        }
        let bytes = write_object(&input);
        let names: Vec<_> = symbol_names(&bytes)
            .into_iter()
            .filter(|name| expected.contains(&name.as_str()))
            .collect();
        assert_eq!(names, expected, "{order:?}, full={full}");
        let header = section_header(&bytes, section_index(&bytes, ".rela.text"));
        let offset = be_u32(&bytes, header + 16) as usize;
        let names = symbol_names(&bytes);
        for (index, expected) in ["second", "first"].into_iter().enumerate() {
            let info = be_u32(&bytes, offset + index * 12 + 4);
            assert_eq!(names[(info >> 8) as usize], expected);
        }
    }
}

#[test]
fn discovered_full_bss_follows_strings_but_precedes_small_data_function_event() {
    for (order, expected) in [
        (
            FunctionSymbolOrder::ReferencesFirst,
            vec!["before", "@5", "second", "first", "after"],
        ),
        (
            FunctionSymbolOrder::FunctionFirst,
            vec!["before", "@5", "first", "after", "second"],
        ),
    ] {
        let mut input = static_frontier_input(0, true);
        input.object_format.local_data_symbols_in_declaration_order = false;
        input.object_format.function_symbol_order = order;
        input.data_objects[0].size = 64;
        let mut string = static_frontier_input(0, false).data_objects.remove(1);
        string.name = "@5";
        string.initial_bytes = Some(vec![b'x', 0, 0, 0]);
        input.data_objects.push(string);
        input.functions[1].string_names = vec!["@5".into()];
        input.functions[1].string_count = 1;
        input.functions[1].relocations.insert(
            0,
            TextRelocation {
                offset: 0,
                elf_type: 6,
                target: RelocationTarget::External("@5".into()),
            },
        );
        let bytes = write_object(&input);
        let names: Vec<_> = symbol_names(&bytes)
            .into_iter()
            .filter(|name| expected.contains(&name.as_str()))
            .collect();
        assert_eq!(names, expected, "{order:?}");
    }
}

#[test]
fn constant_local_initializers_interleave_strings_storage_and_symbols() {
    for (literal_section, local_section) in [
        (".sdata", ".sdata"),
        (".data", ".data"),
        (".rodata", ".rodata"),
        (".data", ".sdata"),
    ] {
        let mut input = static_frontier_input(0, false);
        input.object_format.function_symbol_order = FunctionSymbolOrder::ReferencesFirst;
        input
            .object_format
            .initialized_globals_before_deferred_functions = false;
        input.object_format.data_relocations_use_section_anchors = true;
        input.functions[1].is_static = false;
        input.functions[1].string_names = vec!["@5".into(), "@7".into()];
        input.functions[1].string_count = 2;
        input.data_objects.clear();
        for (name, target) in [("p", "@5"), ("q", "@7"), ("r", "@5")] {
            input.data_objects.push(DataObject {
                name,
                section: Some(local_section.into()),
                initial_bytes: Some(vec![0; 4]),
                static_local_owner: Some(1),
                functions_before: 1,
                relocations: vec![crate::DataRelocation {
                    offset: 0,
                    target: target.into(),
                    addend: 0,
                }],
                ..static_frontier_input(0, false).data_objects.remove(0)
            });
        }
        for (name, bytes) in [("@5", b"one\0"), ("@7", b"two\0")] {
            input.data_objects.push(DataObject {
                name,
                section: Some(literal_section.into()),
                initial_bytes: Some(bytes.to_vec()),
                functions_before: 1,
                ..static_frontier_input(0, false).data_objects.remove(0)
            });
        }
        let bytes = write_object(&input);
        let names = symbol_names(&bytes);
        let local_names: Vec<_> = ["p$", "q$", "r$"]
            .into_iter()
            .map(|prefix| names.iter().find(|name| name.starts_with(prefix)).unwrap())
            .collect();
        let relevant: Vec<_> = names
            .iter()
            .filter(|name| {
                ["before", "@5", "@7", "after"].contains(&name.as_str())
                    || local_names.contains(name)
            })
            .map(|name| name.as_str())
            .collect();
        assert_eq!(
            relevant,
            [
                "before",
                "@5",
                local_names[0],
                "@7",
                local_names[1],
                local_names[2],
                "after"
            ],
            "{literal_section}, {local_section}"
        );
        let expected_offsets = if literal_section == local_section {
            [0, 4, 8, 12, 16]
        } else {
            [0, 0, 4, 4, 8]
        };
        for (name, offset) in ["@5", local_names[0], "@7", local_names[1], local_names[2]]
            .into_iter()
            .zip(expected_offsets)
        {
            assert_eq!(symbol_value_and_size(&bytes, name), (offset, 4));
        }
        if literal_section == ".data" {
            let position = |wanted| names.iter().position(|name| name == wanted).unwrap();
            assert!(position("before") < position("...data.0"));
            assert_eq!(position("...data.0") + 1, position("@5"));
        }
    }
}
