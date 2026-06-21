//! Assembly of a relocatable object, byte-for-byte as mwcceppc emits it. The
//! object holds one or more functions sharing a single `.text`; the Metrowerks
//! `.mwcats.text` section carries one `(0x02000000 | function size, &function)`
//! record per function, each with its relocation, alongside a symbol table (file,
//! section, the anonymous `@N` locals, the undefined externals, and a symbol per
//! function) and the `.comment` record.
//!
//! Float/stack-frame functions add `.sdata2` / `extab` / `extabindex` sections,
//! pooled across all functions in the unit.

use crate::{DataObject, ObjectInput, RelocationTarget};

/// Metrowerks' private section type for `.mwcats.text` (readelf renders it as
/// "LOUSER+0x4a2a82c2").
const SHT_MWCATS: u32 = 0xCA2A_82C2;

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8; // .sbss/.bss: a size but no file content

const SHF_WRITE_EXEC: u32 = 0x6; // ALLOC | EXECINSTR for .text
const SHF_ALLOC: u32 = 0x2; // ALLOC for the unwind tables (read-only data)
const SHF_WRITE_ALLOC: u32 = 0x3; // WRITE | ALLOC for the .sdata2 constant pool
const R_PPC_ADDR32: u32 = 1;

const SHN_ABS: u16 = 0xFFF1;
const SHN_UNDEF: u16 = 0;
const STT_FILE: u8 = 4; // STB_LOCAL (0<<4) | STT_FILE
const STT_SECTION: u8 = 3; // STB_LOCAL | STT_SECTION
const STB_LOCAL_OBJECT: u8 = 1; // STB_LOCAL | STT_OBJECT (the @N unwind entries)
const STB_GLOBAL_FUNC: u8 = (1 << 4) | 2; // STB_GLOBAL | STT_FUNC
const STB_LOCAL_FUNC: u8 = 2; // STB_LOCAL | STT_FUNC (a `static` function)
const STB_GLOBAL_OBJECT: u8 = (1 << 4) | 1; // STB_GLOBAL | STT_OBJECT (a defined global)
const STB_GLOBAL_NOTYPE: u8 = 1 << 4; // STB_GLOBAL | STT_NOTYPE (undefined external)
const STV_HIDDEN: u8 = 2; // st_other visibility for the @N unwind entries

/// The Metrowerks `.comment` record for a plain function. Bytes 12..15 spell the
/// compiler version (`02 04 0X` = 2.4.X) and byte 11 is a format marker that
/// tracks the version line; [`comment_record`] patches them per build. After the
/// fixed 56-byte prefix (ending `…00 00 00 01`) comes one eight-byte record
/// `00 00 00 00 <alignment>` per symbol — every symbol table entry past the null
/// and FILE entries, in order — carrying that symbol's alignment (0 for an
/// undefined external), then a trailing zero word.
const COMMENT_PREFIX: [u8; 56] = [
    b'C', b'o', b'd', b'e', b'W', b'a', b'r', b'r', b'i', b'o', b'r', b'\n', //
    0x02, 0x04, 0x02, 0x01, 0x01, 0x02, 0x00, 0x16, 0x2c, 0x00, 0x00, 0x00, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0x00, 0x00, 0x00, 0x01,
];

/// The `.comment` record for a specific compiler version and build, plus the
/// per-symbol alignment values (one per symbol past null and FILE, in order).
fn comment_record(version: (u8, u8, u8), build: u16, symbol_alignments: &[u32]) -> Vec<u8> {
    let mut record = COMMENT_PREFIX.to_vec();
    // Byte 11 is a format marker: 0x0a for every supported build except GC/2.7
    // (build 108), which bumped it to 0x0b. Bytes 12..15 are the version itself.
    record[11] = if build == 108 { 0x0b } else { 0x0a };
    record[12] = version.0;
    record[13] = version.1;
    record[14] = version.2;
    for &alignment in symbol_alignments {
        record.extend_from_slice(&[0, 0, 0, 0]);
        record.extend_from_slice(&alignment.to_be_bytes());
    }
    record.extend_from_slice(&[0, 0, 0, 0]);
    record
}

const ELF_HEADER_SIZE: u32 = 52;
const SECTION_HEADER_SIZE: u32 = 40;
const SYMBOL_SIZE: usize = 16;

/// One section's header fields plus its payload bytes. The writer lays these out
/// in order; `link`/`info` are resolved section indices.
struct Section {
    name_offset: u32,
    sh_type: u32,
    flags: u32,
    link: u32,
    info: u32,
    align: u32,
    entry_size: u32,
    payload: Vec<u8>,
    /// `sh_size`. Equals `payload.len()` for a section with file content; for a
    /// NOBITS section (`.sbss`/`.bss`) the payload is empty but the size is the
    /// in-memory byte count.
    size: u32,
}

pub fn write_object<'a>(input: &ObjectInput<'a>) -> Vec<u8> {
    let functions = &input.functions;

    // `.text` is the functions concatenated in source order (each function's text
    // size is a multiple of 4, so they pack contiguously). Track each function's
    // byte offset and size for its symbol, relocations, and `.mwcats` record.
    let mut text = Vec::new();
    let mut function_offset: Vec<u32> = Vec::new();
    let mut function_size: Vec<u32> = Vec::new();
    for function in functions {
        function_offset.push(text.len() as u32);
        function_size.push(function.text.len() as u32);
        text.extend_from_slice(function.text);
    }

    let has_text_relocations = functions.iter().any(|function| !function.relocations.is_empty());
    let has_frame = functions.iter().any(|function| function.frame.is_some());
    let has_constants = functions.iter().any(|function| !function.constants.is_empty());
    // Each defined object is routed to a section by const-ness, size, and whether
    // it is initialized: a writable global to `.sdata` (initialized) or `.sbss`
    // (zero), a const one to `.sdata2` (≤ 8 bytes) or `.rodata` (larger). mwcc lays
    // `.sbss` (small zero) out in REVERSE declaration order; every other data
    // section — including `.bss` (large zero) — is FORWARD. (Verified against the
    // real compiler: two small uninitialized scalars reverse, two large ones don't.)
    // Const objects are read-only (`.sdata2`/`.rodata`); writable ones split by the
    // 8-byte small-data threshold: small to `.sdata`/`.sbss`, large to `.data`/`.bss`.
    let section_of = |object: &DataObject| -> &'static str {
        if object.is_const {
            if object.size <= 8 { ".sdata2" } else { ".rodata" }
        } else if object.size <= 8 {
            if object.initial_bytes.is_some() { ".sdata" } else { ".sbss" }
        } else if object.initial_bytes.is_some() {
            ".data"
        } else {
            ".bss"
        }
    };
    let has_sdata = input.data_objects.iter().any(|object| section_of(object) == ".sdata");
    let has_sbss = input.data_objects.iter().any(|object| section_of(object) == ".sbss");
    let has_rodata = input.data_objects.iter().any(|object| section_of(object) == ".rodata");
    let has_const_sdata2 = input.data_objects.iter().any(|object| section_of(object) == ".sdata2");
    let has_file_data = input.data_objects.iter().any(|object| section_of(object) == ".data");
    let has_bss = input.data_objects.iter().any(|object| section_of(object) == ".bss");

    let mut data_section: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    let mut data_offsets: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut data_sizes: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut data_aligns: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut place = |object: &DataObject<'a>, section: &'static str, cursor: &mut u32| {
        let alignment = object.alignment.max(1);
        *cursor = cursor.div_ceil(alignment) * alignment;
        data_section.insert(object.name, section);
        data_offsets.insert(object.name, *cursor);
        data_sizes.insert(object.name, object.size);
        data_aligns.insert(object.name, alignment);
        *cursor += object.size;
    };
    // The const `.sdata2` globals occupy the FRONT of the constant pool (ahead of
    // any function float constants), in forward declaration order.
    let mut sdata2_global_size = 0u32;
    for object in input.data_objects.iter().filter(|object| section_of(object) == ".sdata2") {
        place(object, ".sdata2", &mut sdata2_global_size);
    }
    let mut rodata_size = 0u32;
    for object in input.data_objects.iter().filter(|object| section_of(object) == ".rodata") {
        place(object, ".rodata", &mut rodata_size);
    }
    // Large writable globals: both `.data` (initialized) and `.bss` (zero) are laid
    // out FORWARD. Only the small-data `.sbss` reverses (below) — `.bss` does not.
    let mut file_data_size = 0u32;
    for object in input.data_objects.iter().filter(|object| section_of(object) == ".data") {
        place(object, ".data", &mut file_data_size);
    }
    let mut bss_size = 0u32;
    for object in input.data_objects.iter().filter(|object| section_of(object) == ".bss") {
        place(object, ".bss", &mut bss_size);
    }
    let mut sdata_size = 0u32;
    for object in input.data_objects.iter().filter(|object| section_of(object) == ".sdata") {
        place(object, ".sdata", &mut sdata_size);
    }
    let mut sbss_size = 0u32;
    for object in input.data_objects.iter().rev().filter(|object| section_of(object) == ".sbss") {
        place(object, ".sbss", &mut sbss_size);
    }
    // `.sdata`/`.rodata`/`.data` file bytes: each initialized object's bytes at its
    // offset. (`.sdata2` const-global bytes are laid into the pool below, with the
    // floats; `.bss` is zero-initialized so has no file bytes.)
    let mut sdata = vec![0u8; sdata_size as usize];
    let mut rodata = vec![0u8; rodata_size as usize];
    let mut file_data = vec![0u8; file_data_size as usize];
    for object in &input.data_objects {
        if let Some(bytes) = &object.initial_bytes {
            let offset = data_offsets[object.name] as usize;
            match section_of(object) {
                ".sdata" => sdata[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".rodata" => rodata[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".data" => file_data[offset..offset + bytes.len()].copy_from_slice(bytes),
                _ => {}
            }
        }
    }

    // Jump tables (dense switches) live in a `.data` section: one 4-byte entry per
    // index, every entry filled by an `ADDR32` relocation (so the file bytes are
    // zero). Each table is recorded at its offset; `.data` is 8-aligned.
    let mut jump_data_size = 0u32;
    let mut jump_table_offset: Vec<Option<u32>> = Vec::new();
    for function in functions {
        if let Some(table) = &function.jump_table {
            jump_data_size = jump_data_size.div_ceil(4) * 4;
            jump_table_offset.push(Some(jump_data_size));
            jump_data_size += table.entries.len() as u32 * 4;
        } else {
            jump_table_offset.push(None);
        }
    }
    let has_jump_table = jump_data_size > 0;
    let jump_data = vec![0u8; jump_data_size as usize];

    // `.sdata2` constant pool — the const globals routed here (laid out above) come
    // first, then every function's float constants appended in source order, each at
    // its natural alignment. Record the byte offset of each function's j-th constant.
    let mut sdata2 = vec![0u8; sdata2_global_size as usize];
    for object in &input.data_objects {
        if section_of(object) == ".sdata2" {
            if let Some(bytes) = &object.initial_bytes {
                let offset = data_offsets[object.name] as usize;
                sdata2[offset..offset + bytes.len()].copy_from_slice(bytes);
            }
        }
    }
    let mut constant_offsets: Vec<Vec<u32>> = Vec::new();
    for function in functions {
        let mut offsets = Vec::new();
        for constant in &function.constants {
            let alignment = constant.byte_width as usize;
            while sdata2.len() % alignment != 0 {
                sdata2.push(0);
            }
            offsets.push(sdata2.len() as u32);
            match constant.byte_width {
                8 => sdata2.extend_from_slice(&constant.bits.to_be_bytes()),
                _ => sdata2.extend_from_slice(&(constant.bits as u32).to_be_bytes()),
            }
        }
        constant_offsets.push(offsets);
    }

    // The anonymous `@N` counter, walked once over the functions. Each function's
    // constants are numbered first, then its (hidden) unwind entries; the counter
    // starts at 5, is bumped per function by its conversions/float-branches, and
    // advances by 4 past each function beyond what that function consumed.
    struct FrameNumbers {
        extab: u32,
        extabindex: u32,
        extab_entry_offset: u32,
        extabindex_entry_offset: u32,
    }
    let mut constant_numbers: Vec<Vec<u32>> = Vec::new();
    let mut frame_numbers: Vec<Option<FrameNumbers>> = Vec::new();
    let mut jump_table_numbers: Vec<Option<u32>> = Vec::new();
    let mut extab_payload_offset = 0u32;
    let mut extabindex_payload_offset = 0u32;
    // The functions' anonymous `@N` numbering starts at 5, raised by one per pooled
    // string literal (an anonymous `@N` `.sdata` object): a string is `@1..`, so a
    // function's first constant moves from `@5` to `@(5 + strings)`.
    let pooled_string_count = input.data_objects.iter().filter(|object| object.is_static && object.name.starts_with('@')).count() as u32;
    let mut counter = 5u32 + pooled_string_count;
    for function in functions {
        let mut number = counter + function.anonymous_bump;
        let mut numbers = Vec::new();
        for _ in &function.constants {
            numbers.push(number);
            number += 1;
        }
        constant_numbers.push(numbers);
        // A dense switch's jump table is numbered after the function's internal
        // labels (a label per case, the dispatch, and an explicit `default:`).
        if let Some(table) = &function.jump_table {
            let number_of_table = number + table.anonymous_offset;
            number = number_of_table;
            jump_table_numbers.push(Some(number_of_table));
        } else {
            jump_table_numbers.push(None);
        }
        if function.frame.is_some() {
            let frame = FrameNumbers {
                extab: number,
                extabindex: number + 1,
                extab_entry_offset: extab_payload_offset,
                extabindex_entry_offset: extabindex_payload_offset,
            };
            number += 2;
            extab_payload_offset += 8;
            extabindex_payload_offset += 12;
            frame_numbers.push(Some(frame));
        } else {
            frame_numbers.push(None);
        }
        counter = number + 4;
    }

    // 1. The ordered section-name list (index 0 is the implicit NULL section). The
    //    unwind tables sit right after `.text`, then the `.sdata2` constant pool;
    //    their `.rela` and everything downstream key off this order, by name. A
    //    data-only unit (no functions) omits `.text` and the `.mwcats` machinery.
    let has_functions = !functions.is_empty();
    // A jump table and large writable globals share `.data`; the lowering guarantees
    // they do not co-occur, so one `.data` entry covers both.
    let has_data = has_jump_table || has_file_data;
    let mut order: Vec<&str> = Vec::new();
    if has_functions {
        order.push(".text");
    }
    if has_frame {
        order.push("extab");
        order.push("extabindex");
    }
    // Read-only const data (`.rodata`) then large writable data (`.data`/`.bss`)
    // precede the small-data sections, which in turn precede the `.sdata2` pool.
    if has_rodata {
        order.push(".rodata");
    }
    if has_data {
        order.push(".data");
    }
    if has_bss {
        order.push(".bss");
    }
    if has_sdata {
        order.push(".sdata");
    }
    if has_sbss {
        order.push(".sbss");
    }
    if has_constants || has_const_sdata2 {
        order.push(".sdata2");
    }
    if has_functions {
        order.push(".mwcats.text");
    }
    if has_text_relocations {
        order.push(".rela.text");
    }
    if has_frame {
        order.push(".relaextabindex");
    }
    // The `.rela.*` sections follow their target sections' order, so `.rela.sdata`
    // (→ `.sdata`) precedes `.rela.mwcats.text` (→ `.mwcats.text`, last).
    let has_data_relocs = input.data_objects.iter().any(|object| section_of(object) == ".data" && !object.relocations.is_empty());
    if has_jump_table || has_data_relocs {
        order.push(".rela.data");
    }
    let has_sdata_relocs = input.data_objects.iter().any(|object| section_of(object) == ".sdata" && !object.relocations.is_empty());
    if has_sdata_relocs {
        order.push(".rela.sdata");
    }
    if has_functions {
        order.push(".rela.mwcats.text");
    }
    order.push(".symtab");
    order.push(".strtab");
    order.push(".shstrtab");
    order.push(".comment");
    // Section index of a name (NULL is 0, so the list is offset by one).
    let index_of = |name: &str| order.iter().position(|entry| *entry == name).unwrap() as u32 + 1;

    // 2. Symbols, building `.strtab` alongside. Order: null, FILE, one SECTION
    //    symbol per content section (in section order), the local `@N` entries
    //    grouped by function (constants then unwind), then the GLOBAL run — each
    //    function's not-yet-seen externals followed by the function symbol, in
    //    source order. The first GLOBAL is `sh_info` for `.symtab`.
    let content_sections: Vec<&str> = [".text", "extab", "extabindex", ".rodata", ".data", ".bss", ".sdata", ".sbss", ".sdata2", ".mwcats.text"]
        .into_iter()
        .filter(|name| order.contains(name))
        .collect();
    // The `.comment` trailer carries one record per symbol *after* the null and
    // FILE entries, holding that symbol's alignment (0 for an undefined external).
    // Values are collected here in symbol-emission order.
    let mut comment_values: Vec<u32> = Vec::new();
    let section_align = |name: &str| -> u32 {
        match name {
            ".sdata2" | ".sdata" | ".sbss" | ".data" | ".bss" | ".rodata" => 8,
            _ => 4,
        }
    };
    let mut strtab = StringTable::new();
    let mut symtab = Vec::new();
    write_symbol(&mut symtab, 0, 0, 0, 0, 0, 0); // null
    write_symbol(&mut symtab, strtab.add(input.source_name), 0, 0, STT_FILE, 0, SHN_ABS);
    for name in &content_sections {
        write_symbol(&mut symtab, 0, 0, 0, STT_SECTION, 0, index_of(name) as u16);
        comment_values.push(section_align(name));
    }
    // `static inline` asm helpers (e.g. OSFastCast.h) — a local undefined symbol
    // each, in declaration order, right after the section symbols. `info = 0` is
    // STB_LOCAL | STT_NOTYPE; an undefined symbol has `.comment` alignment 0.
    for name in input.inline_asm_symbols {
        write_symbol(&mut symtab, strtab.add(name), 0, 0, 0, 0, SHN_UNDEF);
        comment_values.push(0);
    }
    // `static` (file-local) data objects: a LOCAL object symbol each, after the
    // inline-asm locals and before the functions' `@N` entries. The INITIALIZED ones
    // (`.sdata`/`.data`) come first in FORWARD declaration order, then the ZERO ones
    // (`.sbss`/`.bss`) in REVERSE — the same split (and same order) the uninitialized
    // globals follow. (Only the common "all static data before any function" shape is
    // produced; the parser defers a static global that follows a function.) Their
    // indices are kept so a function relocation that targets one resolves locally.
    let mut local_data_symbols: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let is_zero_section = |name: &str| matches!(data_section[name], ".sbss" | ".bss");
    // Initialized statics first, FORWARD declaration order.
    for object in &input.data_objects {
        if object.is_static && !is_zero_section(object.name) {
            local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            write_symbol(&mut symtab, strtab.add(object.name), data_offsets[object.name], data_sizes[object.name], STB_LOCAL_OBJECT, 0, section);
            comment_values.push(data_aligns[object.name]);
        }
    }
    // Then zero statics, REVERSE declaration order.
    for object in input.data_objects.iter().rev() {
        if object.is_static && is_zero_section(object.name) {
            local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            write_symbol(&mut symtab, strtab.add(object.name), data_offsets[object.name], data_sizes[object.name], STB_LOCAL_OBJECT, 0, section);
            comment_values.push(data_aligns[object.name]);
        }
    }
    // `static` functions are file-local: a LOCAL `STT_FUNC` symbol each, in
    // declaration order, after the static data and before the functions' `@N`
    // entries (mwcc emits `static int f(){…}` here, ahead of any unwind `@N`). Their
    // symbol indices are recorded by function index so a call relocation resolves to
    // the local symbol; the global run below skips them.
    let mut function_symbols: Vec<u32> = vec![0u32; functions.len()];
    let mut local_function_symbols: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for (index, function) in functions.iter().enumerate() {
        if function.is_static {
            let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
            function_symbols[index] = symbol;
            local_function_symbols.insert(function.name, symbol);
            write_symbol(&mut symtab, strtab.add(function.name), function_offset[index], function_size[index], STB_LOCAL_FUNC, 0, index_of(".text") as u16);
            comment_values.push(4); // a function is 4-aligned
        }
    }
    // Local `@N`: per function, its pooled constants (visible `.sdata2` objects)
    // then its hidden unwind entries.
    let mut constant_symbols: Vec<Vec<u32>> = Vec::new();
    let mut extab_entry_symbols: Vec<u32> = Vec::new();
    let mut jump_table_symbols: Vec<u32> = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let mut symbols = Vec::new();
        for (constant_index, constant) in function.constants.iter().enumerate() {
            symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let name = strtab.add(&format!("@{}", constant_numbers[index][constant_index]));
            write_symbol(&mut symtab, name, constant_offsets[index][constant_index], constant.byte_width as u32, STB_LOCAL_OBJECT, 0, index_of(".sdata2") as u16);
            comment_values.push(constant.byte_width as u32);
        }
        constant_symbols.push(symbols);
        if let Some(frame) = &frame_numbers[index] {
            extab_entry_symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let extab_name = strtab.add(&format!("@{}", frame.extab));
            write_symbol(&mut symtab, extab_name, frame.extab_entry_offset, 8, STB_LOCAL_OBJECT, STV_HIDDEN, index_of("extab") as u16);
            let extabindex_name = strtab.add(&format!("@{}", frame.extabindex));
            write_symbol(&mut symtab, extabindex_name, frame.extabindex_entry_offset, 12, STB_LOCAL_OBJECT, STV_HIDDEN, index_of("extabindex") as u16);
            // The unwind entries are 4-aligned objects.
            comment_values.push(4);
            comment_values.push(4);
        } else {
            extab_entry_symbols.push(0);
        }
        // The jump table is a 4-aligned local `@N` object in `.data`.
        if let Some(number) = jump_table_numbers[index] {
            jump_table_symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let name = strtab.add(&format!("@{}", number));
            let size = function.jump_table.as_ref().unwrap().entries.len() as u32 * 4;
            write_symbol(&mut symtab, name, jump_table_offset[index].unwrap(), size, STB_LOCAL_OBJECT, 0, index_of(".data") as u16);
            comment_values.push(4);
        } else {
            jump_table_symbols.push(0);
        }
    }
    // The GLOBAL run. mwcc emits symbols in source-encounter order, which for the
    // common shape (data declared before functions) means:
    //   1. the INITIALIZED (.sdata) defined globals, in declaration order, up front;
    //   2. then per function its newly-referenced symbols (an undefined external,
    //      or a referenced .sbss defined global) followed by the function symbol;
    //   3. then any still-unreferenced (.sbss) defined global, in declaration order.
    // (.sdata globals appear regardless of reference; .sbss ones follow the
    // reference order, trailing when unused.)
    let first_global_index = (symtab.len() / SYMBOL_SIZE) as u32;
    let mut global_symbols: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    // The always-present initialized sections (`.sdata`/`.data`, and the read-only
    // `.sdata2`/`.rodata`) emit their symbols up front in declaration order; the
    // zero `.sbss`/`.bss` objects instead follow reference order (handled below).
    for object in &input.data_objects {
        // `static` objects already have their LOCAL symbol; only exported globals
        // appear in this run.
        if object.is_static {
            continue;
        }
        let section_name = data_section[object.name];
        if matches!(section_name, ".sdata" | ".data" | ".sdata2" | ".rodata") {
            global_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(section_name) as u16;
            write_symbol(&mut symtab, strtab.add(object.name), data_offsets[object.name], data_sizes[object.name], STB_GLOBAL_OBJECT, 0, section);
            comment_values.push(data_aligns[object.name]);
            // mwcc emits each pointer global's relocation targets immediately after
            // it (`p, &a; q, &b`), not all targets at the end — and within one object
            // (a pointer array `{&a, &b}`) in REVERSE element order (`t, &b, &a`). A
            // target defined in this unit resolves to its own data symbol; an
            // external one is undefined.
            for relocation in object.relocations.iter().rev() {
                let target = relocation.target.as_str();
                if global_symbols.contains_key(target) || local_data_symbols.contains_key(target) {
                    continue;
                }
                global_symbols.insert(target, (symtab.len() / SYMBOL_SIZE) as u32);
                if let Some(&offset) = data_offsets.get(target) {
                    write_symbol(&mut symtab, strtab.add(target), offset, data_sizes[target], STB_GLOBAL_OBJECT, 0, index_of(data_section[target]) as u16);
                    comment_values.push(data_aligns[target]);
                } else {
                    write_symbol(&mut symtab, strtab.add(target), 0, 0, STB_GLOBAL_NOTYPE, 0, SHN_UNDEF);
                    comment_values.push(0);
                }
            }
        }
    }
    for (index, function) in functions.iter().enumerate() {
        // Assign this function's referenced externals in mwcc's symbol-table order
        // (its AST `symbol_order`) for the names it lists, then any remaining in
        // relocation order so nothing is missed. `.text` reference (offset) order
        // does not match mwcc's symbol order, so we cannot key off the relocations.
        let external_targets: std::collections::HashSet<&str> = function
            .relocations
            .iter()
            .filter_map(|relocation| match &relocation.target {
                RelocationTarget::External(name) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let mut ordered: Vec<&str> = Vec::new();
        let mut listed = std::collections::HashSet::new();
        for name in &function.symbol_order {
            if external_targets.contains(name.as_str()) && listed.insert(name.as_str()) {
                ordered.push(name.as_str());
            }
        }
        for relocation in &function.relocations {
            if let RelocationTarget::External(name) = &relocation.target {
                if listed.insert(name.as_str()) {
                    ordered.push(name.as_str());
                }
            }
        }
        for name in ordered {
            // A reference to a `static` object or function resolves to its existing
            // LOCAL symbol; it is not (re)emitted in the global run.
            if global_symbols.contains_key(name) || local_data_symbols.contains_key(name) || local_function_symbols.contains_key(name) {
                continue;
            }
            global_symbols.insert(name, (symtab.len() / SYMBOL_SIZE) as u32);
            if let Some(&offset) = data_offsets.get(name) {
                let section = index_of(data_section[name]) as u16;
                write_symbol(&mut symtab, strtab.add(name), offset, data_sizes[name], STB_GLOBAL_OBJECT, 0, section);
                comment_values.push(data_aligns[name]);
            } else {
                write_symbol(&mut symtab, strtab.add(name), 0, 0, STB_GLOBAL_NOTYPE, 0, SHN_UNDEF);
                comment_values.push(0); // an undefined external has no alignment
            }
        }
        // A `static` function already has its LOCAL symbol (emitted above); only its
        // newly-referenced externals appear in this run, not the function symbol.
        if !function.is_static {
            function_symbols[index] = (symtab.len() / SYMBOL_SIZE) as u32;
            write_symbol(&mut symtab, strtab.add(function.name), function_offset[index], function_size[index], STB_GLOBAL_FUNC, 0, index_of(".text") as u16);
            comment_values.push(4); // a function is 4-aligned
        }
    }
    // Still-unreferenced (.sbss/.bss) defined globals trail the functions, in
    // REVERSE declaration order (verified: `int a;b;c;d;e;` -> `e d c b a`, and a
    // mixed .bss/.sbss set reverses too, independent of section). `static` objects
    // are local and never appear here.
    for object in input.data_objects.iter().rev() {
        if !object.is_static && !global_symbols.contains_key(object.name) {
            global_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            write_symbol(&mut symtab, strtab.add(object.name), data_offsets[object.name], data_sizes[object.name], STB_GLOBAL_OBJECT, 0, section);
            comment_values.push(data_aligns[object.name]);
        }
    }
    // The `.comment` trailer is now fully determined by the symbol alignments.
    let comment = comment_record(input.version, input.build, &comment_values);

    // 3. Relocation payloads (now that symbol indices are fixed). Each function's
    //    `.text` relocations are rebased by its `.text` offset; a relocation
    //    targets either an external or one of that function's pooled constants.
    let mut rela_text = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        for relocation in &function.relocations {
            let symbol = match &relocation.target {
                // A `static` target is a local data or function symbol; everything
                // else is a global/external symbol.
                RelocationTarget::External(name) => *local_data_symbols
                    .get(name.as_str())
                    .or_else(|| local_function_symbols.get(name.as_str()))
                    .unwrap_or_else(|| &global_symbols[name.as_str()]),
                RelocationTarget::Constant(constant_index) => constant_symbols[index][*constant_index],
                RelocationTarget::JumpTable => jump_table_symbols[index],
            };
            write_rela(&mut rela_text, function_offset[index] + relocation.offset, symbol, relocation.elf_type, 0);
        }
    }
    // `.rela.data` — each jump-table entry is an `ADDR32` to its function with the
    // case body's byte offset as the addend.
    let mut rela_data = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        if let Some(table) = &function.jump_table {
            let base = jump_table_offset[index].unwrap();
            for (entry_index, &body_offset) in table.entries.iter().enumerate() {
                write_rela(&mut rela_data, base + entry_index as u32 * 4, function_symbols[index], R_PPC_ADDR32, body_offset);
            }
        }
    }
    let mut rela_extabindex = Vec::new();
    for (index, frame) in frame_numbers.iter().enumerate() {
        if let Some(frame) = frame {
            write_rela(&mut rela_extabindex, frame.extabindex_entry_offset, function_symbols[index], R_PPC_ADDR32, 0); // -> the function
            write_rela(&mut rela_extabindex, frame.extabindex_entry_offset + 8, extab_entry_symbols[index], R_PPC_ADDR32, 0); // -> its extab entry
        }
    }
    let mut rela_mwcats = Vec::new();
    for (index, _) in functions.iter().enumerate() {
        write_rela(&mut rela_mwcats, index as u32 * 8 + 4, function_symbols[index], R_PPC_ADDR32, 0);
    }
    // `.rela.sdata`/`.rela.data` — a pointer global's `ADDR32` to the symbol it
    // points at, at the object's offset plus the relocation's own offset (plus
    // addend). Within one object (a pointer array) both the target symbols AND the
    // relocation entries run in REVERSE element order. `.sdata` for small pointers,
    // `.data` for large arrays.
    let mut rela_sdata = Vec::new();
    let resolve_data_target = |name: &str| -> u32 {
        *local_data_symbols.get(name).unwrap_or_else(|| &global_symbols[name])
    };
    for object in &input.data_objects {
        if data_section[object.name] != ".sdata" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(&mut rela_sdata, data_offsets[object.name] + relocation.offset, resolve_data_target(&relocation.target), R_PPC_ADDR32, relocation.addend as u32);
        }
    }
    for object in &input.data_objects {
        if data_section[object.name] != ".data" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(&mut rela_data, data_offsets[object.name] + relocation.offset, resolve_data_target(&relocation.target), R_PPC_ADDR32, relocation.addend as u32);
        }
    }

    // 4. Content payloads. One `.mwcats` record per function: `(0x02000000 | its
    //    text size, &function)`. The unwind header is a deterministic function of
    //    the saved-register shape; each `extabindex` entry is (function, function
    //    size, extab entry).
    let mut mwcats = Vec::new();
    for (index, _) in functions.iter().enumerate() {
        write_u32(&mut mwcats, 0x0200_0000 | function_size[index]);
        write_u32(&mut mwcats, 0);
    }
    let mut extab = Vec::new();
    let mut extabindex = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        if let Some(frame) = &function.frame {
            write_u32(&mut extab, frame.extab_header);
            write_u32(&mut extab, 0);
            write_u32(&mut extabindex, 0); // -> the function
            write_u32(&mut extabindex, function_size[index]);
            write_u32(&mut extabindex, 0); // -> the extab entry
        }
    }

    // 5. `.shstrtab` — section names in section order; record each name's offset.
    let mut shstrtab = StringTable::new();
    // With the small-data area off (`-sdata 0`), the defined-data sections are
    // named `.bss`/`.data` rather than `.sbss`/`.sdata` (identical otherwise). The
    // name only appears in `.shstrtab`, so map it here and keep the internal keys.
    let name_offsets: Vec<u32> = order
        .iter()
        .map(|name| {
            let display = match *name {
                ".sbss" if !input.small_data => ".bss",
                ".sdata" if !input.small_data => ".data",
                other => other,
            };
            shstrtab.add(display)
        })
        .collect();
    let offset_of = |name: &str| name_offsets[order.iter().position(|entry| *entry == name).unwrap()];

    // 6. Assemble the full section table (NULL first), each with its payload.
    let symtab_section = index_of(".symtab");
    let mut sections = vec![Section { name_offset: 0, sh_type: 0, flags: 0, link: 0, info: 0, align: 0, entry_size: 0, payload: Vec::new(), size: 0 }];
    // `mem_size` is the in-memory size; it overrides `payload.len()` only when the
    // payload is empty, which is how a NOBITS section (`.sbss`) carries a size with
    // no file bytes. Every other section passes 0 and takes its payload length.
    let mut push = |name: &str, sh_type, flags, link, info, align, entry_size, payload: Vec<u8>, mem_size: u32| {
        let size = if payload.is_empty() { mem_size } else { payload.len() as u32 };
        sections.push(Section { name_offset: offset_of(name), sh_type, flags, link, info, align, entry_size, payload, size });
    };
    if has_functions {
        push(".text", SHT_PROGBITS, SHF_WRITE_EXEC, 0, 0, 4, 0, text.to_vec(), 0);
    }
    if has_frame {
        push("extab", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, extab, 0);
        push("extabindex", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, extabindex, 0);
    }
    // `.rodata` (read-only const data) then the large writable `.data`/`.bss`
    // precede the small-data sections, matching the section-name order above.
    if has_rodata {
        push(".rodata", SHT_PROGBITS, SHF_ALLOC, 0, 0, 8, 0, rodata, 0);
    }
    if has_data {
        // `.data` holds either a function's jump table or the large initialized
        // globals (the lowering keeps them from co-occurring).
        let payload = if has_jump_table { jump_data } else { file_data };
        push(".data", SHT_PROGBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, payload, 0);
    }
    if has_bss {
        // `.bss` is NOBITS (large zero-initialized globals): a size, no file bytes.
        push(".bss", SHT_NOBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, Vec::new(), bss_size);
    }
    // Defined small data (`.sdata`/`.sbss`) precedes the read-only constant pool
    // (`.sdata2`), matching the section-name order above.
    if has_sdata {
        // `.sdata` holds the initialized values as file bytes.
        push(".sdata", SHT_PROGBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, sdata, 0);
    }
    if has_sbss {
        // `.sbss` is NOBITS: no file bytes, but `sh_size` is the in-memory size.
        push(".sbss", SHT_NOBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, Vec::new(), sbss_size);
    }
    if has_constants || has_const_sdata2 {
        push(".sdata2", SHT_PROGBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, sdata2, 0);
    }
    if has_functions {
        push(".mwcats.text", SHT_MWCATS, 0, index_of(".text"), 0, 4, 1, mwcats, 0);
    }
    if has_text_relocations {
        push(".rela.text", SHT_RELA, 0, symtab_section, index_of(".text"), 4, 12, rela_text, 0);
    }
    if has_frame {
        push(".relaextabindex", SHT_RELA, 0, symtab_section, index_of("extabindex"), 4, 12, rela_extabindex, 0);
    }
    if has_jump_table || has_data_relocs {
        push(".rela.data", SHT_RELA, 0, symtab_section, index_of(".data"), 4, 12, rela_data, 0);
    }
    if has_sdata_relocs {
        push(".rela.sdata", SHT_RELA, 0, symtab_section, index_of(".sdata"), 4, 12, rela_sdata, 0);
    }
    if has_functions {
        push(".rela.mwcats.text", SHT_RELA, 0, symtab_section, index_of(".mwcats.text"), 4, 12, rela_mwcats, 0);
    }
    push(".symtab", SHT_SYMTAB, 0, index_of(".strtab"), first_global_index, 4, 16, symtab, 0);
    // Metrowerks stamps string tables with sh_entsize = 1.
    push(".strtab", SHT_STRTAB, 0, 0, 0, 1, 1, strtab.bytes, 0);
    push(".shstrtab", SHT_STRTAB, 0, 0, 0, 1, 1, shstrtab.bytes, 0);
    push(".comment", SHT_PROGBITS, 0, 0, 0, 1, 1, comment.to_vec(), 0);

    // 7. File offsets — sections pack contiguously (all word-aligned sections have
    //    word-aligned sizes); the section-header table is padded to 8.
    let mut offset = ELF_HEADER_SIZE;
    let offsets: Vec<u32> = sections
        .iter()
        .enumerate()
        .map(|(index, section)| {
            // The NULL section has no file presence — its `sh_offset` stays 0.
            if index == 0 {
                return 0;
            }
            // Honour each section's alignment (e.g. `.sdata2` is 8-aligned, so it
            // may need padding after a `.text` whose size is not a multiple of 8).
            let align = section.align.max(1);
            offset = (offset + align - 1) / align * align;
            let here = offset;
            offset += section.payload.len() as u32;
            here
        })
        .collect();
    let section_headers_offset = align8(offset);

    // 8. Emit: header, payloads, padding, section headers.
    let mut output = Vec::new();
    write_elf_header(&mut output, section_headers_offset, sections.len() as u16, index_of(".shstrtab") as u16);
    for (section, &section_offset) in sections.iter().zip(&offsets) {
        // Pad to the section's aligned offset, then emit its payload.
        while output.len() < section_offset as usize {
            output.push(0);
        }
        output.extend_from_slice(&section.payload);
    }
    while output.len() < section_headers_offset as usize {
        output.push(0);
    }
    for (section, &section_offset) in sections.iter().zip(&offsets) {
        write_section_header(
            &mut output, section.name_offset, section.sh_type, section.flags, section_offset, section.size,
            section.link, section.info, section.align, section.entry_size,
        );
    }
    output
}

/// A null-terminated string table that hands back each string's offset.
struct StringTable {
    bytes: Vec<u8>,
}
impl StringTable {
    fn new() -> Self {
        StringTable { bytes: vec![0] }
    }
    fn add(&mut self, value: &str) -> u32 {
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        offset
    }
}

fn write_elf_header(output: &mut Vec<u8>, section_headers_offset: u32, section_count: u16, shstrndx: u16) {
    output.extend_from_slice(&[0x7f, b'E', b'L', b'F']);
    output.push(1); // ELFCLASS32
    output.push(2); // ELFDATA2MSB (big-endian)
    output.push(1); // EV_CURRENT
    output.extend_from_slice(&[0u8; 9]); // e_ident padding
    write_u16(output, 1); // e_type = ET_REL
    write_u16(output, 20); // e_machine = EM_PPC
    write_u32(output, 1); // e_version
    write_u32(output, 0); // e_entry
    write_u32(output, 0); // e_phoff
    write_u32(output, section_headers_offset); // e_shoff
    write_u32(output, 0x8000_0000); // e_flags = EMB bit
    write_u16(output, ELF_HEADER_SIZE as u16); // e_ehsize
    write_u16(output, 0); // e_phentsize
    write_u16(output, 0); // e_phnum
    write_u16(output, SECTION_HEADER_SIZE as u16); // e_shentsize
    write_u16(output, section_count); // e_shnum
    write_u16(output, shstrndx); // e_shstrndx (.shstrtab)
}

fn align8(value: u32) -> u32 {
    (value + 7) & !7
}
fn write_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}
fn write_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn write_symbol(output: &mut Vec<u8>, name: u32, value: u32, size: u32, info: u8, other: u8, section_index: u16) {
    write_u32(output, name);
    write_u32(output, value);
    write_u32(output, size);
    output.push(info);
    output.push(other);
    write_u16(output, section_index);
}

fn write_rela(output: &mut Vec<u8>, offset: u32, symbol: u32, kind: u32, addend: u32) {
    write_u32(output, offset);
    write_u32(output, (symbol << 8) | kind);
    write_u32(output, addend);
}

#[allow(clippy::too_many_arguments)]
fn write_section_header(
    output: &mut Vec<u8>,
    name: u32,
    section_type: u32,
    flags: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    alignment: u32,
    entry_size: u32,
) {
    write_u32(output, name);
    write_u32(output, section_type);
    write_u32(output, flags);
    write_u32(output, 0); // sh_addr
    write_u32(output, offset);
    write_u32(output, size);
    write_u32(output, link);
    write_u32(output, info);
    write_u32(output, alignment);
    write_u32(output, entry_size);
}
