use super::*;

fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
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
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
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
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
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
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
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
            initialized_globals_before_deferred_functions: false,
            local_data_symbols_in_declaration_order: false,
            small_zero_statics_in_declaration_order: false,
            rodata_anchor_before_data_symbols: false,
            rodata_anchor_comment_flags: 0,
            data_relocations_use_section_anchors: true,
            data_anchor_comment_flags: 0,
            initial_anonymous_counter: 1,
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
