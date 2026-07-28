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
        phantom_externals: Vec::new(),
        post_constant_bump: 0,
        post_function_anonymous_bump: None,
        string_count: 0,
        string_number_after_constants: None,
        string_number_after_rodata: None,
        string_names: Vec::new(),
        jump_tables: Vec::new(),
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
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            small_zero_data_in_declaration_order: false,
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
fn nonadvancing_analysis_constants_trail_function_constant_pools() {
    let residue = DataObject {
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
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    };
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
        phantom_externals: Vec::new(),
        post_constant_bump: 0,
        post_function_anonymous_bump: None,
        string_count: 0,
        string_number_after_constants: None,
        string_number_after_rodata: None,
        string_names: Vec::new(),
        jump_tables: Vec::new(),
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
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            small_zero_data_in_declaration_order: false,
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
        data_objects: vec![residue],
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
        &[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0xaa, 0xbb]
    );
    assert_eq!(symbol_value_and_size(&object, "@1"), (0, 8));
    assert_eq!(symbol_value_and_size(&object, "@190"), (8, 2));
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
        },
        &[],
    );
    let disabled = comment_record(
        CommentFormat {
            marker: 0x08,
            version: (2, 3, 0),
            pooling_enabled: false,
        },
        &[],
    );
    assert_eq!(enabled[11], 0x08);
    assert_eq!(&enabled[12..16], &[2, 3, 0, 1]);
    assert_eq!(enabled[16], 1);
    assert_eq!(disabled[16], 0);
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
    };
    let object = write_object(&ObjectInput {
        source_name: "class.cpp",
        object_format: crate::ObjectFormat {
            comment: CommentFormat {
                marker: 8,
                version: (2, 4, 7),
                pooling_enabled: true,
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::Deferred,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            small_zero_data_in_declaration_order: false,
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
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            small_zero_data_in_declaration_order: false,
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
            },
            emb_sda21_offset: 0,
            code_alignment: 4,
            sdata2_writable: false,
            function_symbol_order: FunctionSymbolOrder::ReferencesFirst,
            weak_vtable_function_symbol_tail: false,
            owned_rtti_closure_relocation_order: false,
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            small_zero_data_in_declaration_order: false,
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
