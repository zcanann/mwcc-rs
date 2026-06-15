//! Assembly of a relocatable object, byte-for-byte as mwcceppc emits it. The
//! object holds one or more functions sharing a single `.text`; the Metrowerks
//! `.mwcats.text` section carries one `(0x02000000 | function size, &function)`
//! record per function, each with its relocation, alongside a symbol table (file,
//! section, the anonymous `@N` locals, the undefined externals, and a symbol per
//! function) and the `.comment` record.
//!
//! Float/stack-frame functions add `.sdata2` / `extab` / `extabindex` sections,
//! pooled across all functions in the unit.

use crate::{ObjectInput, RelocationTarget};

/// Metrowerks' private section type for `.mwcats.text` (readelf renders it as
/// "LOUSER+0x4a2a82c2").
const SHT_MWCATS: u32 = 0xCA2A_82C2;

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;

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
const STB_GLOBAL_NOTYPE: u8 = 1 << 4; // STB_GLOBAL | STT_NOTYPE (undefined external)
const STV_HIDDEN: u8 = 2; // st_other visibility for the @N unwind entries

/// The Metrowerks `.comment` record for a plain function. Bytes 12..15 spell the
/// compiler version (`02 04 0X` = 2.4.X) and byte 11 is a format marker that
/// tracks the version line; [`comment_record`] patches them per build. After the
/// fixed 56-byte prefix (ending `…00 00 00 01`) come `function_count + 2`
/// eight-byte records of `00 00 00 00 00 00 00 04`, then a trailing zero word, so
/// the record grows by eight bytes per additional function. This array is the
/// single-function form (three records); [`comment_record`] reuses its prefix.
const COMMENT_BASE: [u8; 84] = [
    b'C', b'o', b'd', b'e', b'W', b'a', b'r', b'r', b'i', b'o', b'r', b'\n', //
    0x02, 0x04, 0x02, 0x01, 0x01, 0x02, 0x00, 0x16, 0x2c, 0x00, 0x00, 0x00, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00,
];

/// The fixed prefix length: the banner, version line, and framing word ending
/// `00 00 00 01` (the first 56 bytes of [`COMMENT_BASE`]).
const COMMENT_PREFIX_LEN: usize = 56;

/// The `.comment` record for a specific compiler version, build, and function
/// count.
fn comment_record(version: (u8, u8, u8), build: u16, function_count: usize) -> Vec<u8> {
    let mut record = COMMENT_BASE[..COMMENT_PREFIX_LEN].to_vec();
    // Byte 11 is a format marker: 0x0a for every supported build except GC/2.7
    // (build 108), which bumped it to 0x0b. Bytes 12..15 are the version itself.
    record[11] = if build == 108 { 0x0b } else { 0x0a };
    record[12] = version.0;
    record[13] = version.1;
    record[14] = version.2;
    // One eight-byte `…04` record per function plus two fixed framing records,
    // then a trailing zero word.
    for _ in 0..function_count + 2 {
        record.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 4]);
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
}

pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    let functions = &input.functions;
    let comment = comment_record(input.version, input.build, functions.len());

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

    // External symbols referenced by any function's relocations, in first-
    // appearance order across the whole unit (the order mwcc emits them in the
    // GLOBAL symbol run, interleaved with the function symbols).
    let mut externals: Vec<&str> = Vec::new();
    for function in functions {
        for relocation in &function.relocations {
            if let RelocationTarget::External(symbol) = &relocation.target {
                if !externals.iter().any(|name| *name == symbol.as_str()) {
                    externals.push(symbol);
                }
            }
        }
    }
    let has_text_relocations = functions.iter().any(|function| !function.relocations.is_empty());
    let has_frame = functions.iter().any(|function| function.frame.is_some());
    let has_constants = functions.iter().any(|function| !function.constants.is_empty());

    // `.sdata2` constant pool — every function's constants appended in source
    // order, each at its natural alignment. Record the byte offset of each
    // function's j-th constant.
    let mut sdata2 = Vec::new();
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
    let mut extab_payload_offset = 0u32;
    let mut extabindex_payload_offset = 0u32;
    let mut counter = 5u32;
    for function in functions {
        let mut number = counter + function.anonymous_bump;
        let mut numbers = Vec::new();
        for _ in &function.constants {
            numbers.push(number);
            number += 1;
        }
        constant_numbers.push(numbers);
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
    //    their `.rela` and everything downstream key off this order, by name.
    let mut order: Vec<&str> = vec![".text"];
    if has_frame {
        order.push("extab");
        order.push("extabindex");
    }
    if has_constants {
        order.push(".sdata2");
    }
    order.push(".mwcats.text");
    if has_text_relocations {
        order.push(".rela.text");
    }
    if has_frame {
        order.push(".relaextabindex");
    }
    order.push(".rela.mwcats.text");
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
    let content_sections: Vec<&str> = [".text", "extab", "extabindex", ".sdata2", ".mwcats.text"]
        .into_iter()
        .filter(|name| order.contains(name))
        .collect();
    let mut strtab = StringTable::new();
    let mut symtab = Vec::new();
    write_symbol(&mut symtab, 0, 0, 0, 0, 0, 0); // null
    write_symbol(&mut symtab, strtab.add(input.source_name), 0, 0, STT_FILE, 0, SHN_ABS);
    for name in &content_sections {
        write_symbol(&mut symtab, 0, 0, 0, STT_SECTION, 0, index_of(name) as u16);
    }
    // Local `@N`: per function, its pooled constants (visible `.sdata2` objects)
    // then its hidden unwind entries.
    let mut constant_symbols: Vec<Vec<u32>> = Vec::new();
    let mut extab_entry_symbols: Vec<u32> = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let mut symbols = Vec::new();
        for (constant_index, constant) in function.constants.iter().enumerate() {
            symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let name = strtab.add(&format!("@{}", constant_numbers[index][constant_index]));
            write_symbol(&mut symtab, name, constant_offsets[index][constant_index], constant.byte_width as u32, STB_LOCAL_OBJECT, 0, index_of(".sdata2") as u16);
        }
        constant_symbols.push(symbols);
        if let Some(frame) = &frame_numbers[index] {
            extab_entry_symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let extab_name = strtab.add(&format!("@{}", frame.extab));
            write_symbol(&mut symtab, extab_name, frame.extab_entry_offset, 8, STB_LOCAL_OBJECT, STV_HIDDEN, index_of("extab") as u16);
            let extabindex_name = strtab.add(&format!("@{}", frame.extabindex));
            write_symbol(&mut symtab, extabindex_name, frame.extabindex_entry_offset, 12, STB_LOCAL_OBJECT, STV_HIDDEN, index_of("extabindex") as u16);
        } else {
            extab_entry_symbols.push(0);
        }
    }
    // The GLOBAL run: walk functions in order, emitting each newly-referenced
    // external just before the function symbol that first uses it.
    let first_global_index = (symtab.len() / SYMBOL_SIZE) as u32;
    let mut external_symbols: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut function_symbols: Vec<u32> = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        for relocation in &function.relocations {
            if let RelocationTarget::External(name) = &relocation.target {
                if !external_symbols.contains_key(name.as_str()) {
                    let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
                    external_symbols.insert(name.as_str(), symbol);
                    write_symbol(&mut symtab, strtab.add(name), 0, 0, STB_GLOBAL_NOTYPE, 0, SHN_UNDEF);
                }
            }
        }
        function_symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
        write_symbol(&mut symtab, strtab.add(function.name), function_offset[index], function_size[index], STB_GLOBAL_FUNC, 0, index_of(".text") as u16);
    }

    // 3. Relocation payloads (now that symbol indices are fixed). Each function's
    //    `.text` relocations are rebased by its `.text` offset; a relocation
    //    targets either an external or one of that function's pooled constants.
    let mut rela_text = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        for relocation in &function.relocations {
            let symbol = match &relocation.target {
                RelocationTarget::External(name) => external_symbols[name.as_str()],
                RelocationTarget::Constant(constant_index) => constant_symbols[index][*constant_index],
            };
            write_rela(&mut rela_text, function_offset[index] + relocation.offset, symbol, relocation.elf_type, 0);
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
    let name_offsets: Vec<u32> = order.iter().map(|name| shstrtab.add(name)).collect();
    let offset_of = |name: &str| name_offsets[order.iter().position(|entry| *entry == name).unwrap()];

    // 6. Assemble the full section table (NULL first), each with its payload.
    let symtab_section = index_of(".symtab");
    let mut sections = vec![Section { name_offset: 0, sh_type: 0, flags: 0, link: 0, info: 0, align: 0, entry_size: 0, payload: Vec::new() }];
    let mut push = |name: &str, sh_type, flags, link, info, align, entry_size, payload: Vec<u8>| {
        sections.push(Section { name_offset: offset_of(name), sh_type, flags, link, info, align, entry_size, payload });
    };
    push(".text", SHT_PROGBITS, SHF_WRITE_EXEC, 0, 0, 4, 0, text.to_vec());
    if has_frame {
        push("extab", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, extab);
        push("extabindex", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, extabindex);
    }
    if has_constants {
        push(".sdata2", SHT_PROGBITS, SHF_WRITE_ALLOC, 0, 0, 8, 0, sdata2);
    }
    push(".mwcats.text", SHT_MWCATS, 0, index_of(".text"), 0, 4, 1, mwcats);
    if has_text_relocations {
        push(".rela.text", SHT_RELA, 0, symtab_section, index_of(".text"), 4, 12, rela_text);
    }
    if has_frame {
        push(".relaextabindex", SHT_RELA, 0, symtab_section, index_of("extabindex"), 4, 12, rela_extabindex);
    }
    push(".rela.mwcats.text", SHT_RELA, 0, symtab_section, index_of(".mwcats.text"), 4, 12, rela_mwcats);
    push(".symtab", SHT_SYMTAB, 0, index_of(".strtab"), first_global_index, 4, 16, symtab);
    // Metrowerks stamps string tables with sh_entsize = 1.
    push(".strtab", SHT_STRTAB, 0, 0, 0, 1, 1, strtab.bytes);
    push(".shstrtab", SHT_STRTAB, 0, 0, 0, 1, 1, shstrtab.bytes);
    push(".comment", SHT_PROGBITS, 0, 0, 0, 1, 1, comment.to_vec());

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
        let size = section.payload.len() as u32;
        write_section_header(
            &mut output, section.name_offset, section.sh_type, section.flags, section_offset, size,
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
