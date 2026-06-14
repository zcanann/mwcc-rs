//! Assembly of a single-function relocatable object, byte-for-byte as mwcceppc
//! emits it. Every object carries `.text`, the Metrowerks `.mwcats.text` record
//! (a `(0x02000000 | text_size, &function)` pair) with its relocation, a symbol
//! table (file, section, and function symbols), and the `.comment` record.
//!
//! Float/stack-frame functions add `.sdata2` / `extab` / `extabindex` sections;
//! those are the next layer and not emitted yet.

use crate::ObjectInput;

/// Metrowerks' private section type for `.mwcats.text` (readelf renders it as
/// "LOUSER+0x4a2a82c2").
const SHT_MWCATS: u32 = 0xCA2A_82C2;

const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_RELA: u32 = 4;

const SHF_WRITE_EXEC: u32 = 0x6; // ALLOC | EXECINSTR for .text
const R_PPC_ADDR32: u32 = 1;

const SHN_ABS: u16 = 0xFFF1;
const SHN_UNDEF: u16 = 0;
const STT_FILE: u8 = 4; // STB_LOCAL (0<<4) | STT_FILE
const STT_SECTION: u8 = 3; // STB_LOCAL | STT_SECTION
const STB_GLOBAL_FUNC: u8 = (1 << 4) | 2; // STB_GLOBAL | STT_FUNC
const STB_GLOBAL_NOTYPE: u8 = 1 << 4; // STB_GLOBAL | STT_NOTYPE (undefined external)

/// The Metrowerks `.comment` record for a plain function. Bytes 12..15 spell the
/// compiler version (`02 04 0X` = 2.4.X) and byte 11 is a format marker that
/// tracks the version line; [`comment_record`] patches them per build. The record
/// grows for objects with extra sections (float/frame); that variant arrives with
/// those sections.
const COMMENT_BASE: [u8; 84] = [
    b'C', b'o', b'd', b'e', b'W', b'a', b'r', b'r', b'i', b'o', b'r', b'\n', //
    0x02, 0x04, 0x02, 0x01, 0x01, 0x02, 0x00, 0x16, 0x2c, 0x00, 0x00, 0x00, //
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, //
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00,
];

/// The `.comment` record for a specific compiler version and build.
fn comment_record(version: (u8, u8, u8), build: u16) -> [u8; 84] {
    let mut record = COMMENT_BASE;
    // Byte 11 is a format marker: 0x0a for every supported build except GC/2.7
    // (build 108), which bumped it to 0x0b. Bytes 12..15 are the version itself.
    record[11] = if build == 108 { 0x0b } else { 0x0a };
    record[12] = version.0;
    record[13] = version.1;
    record[14] = version.2;
    record
}

const ELF_HEADER_SIZE: u32 = 52;
const SECTION_HEADER_SIZE: u32 = 40;

pub fn write_object(input: &ObjectInput<'_>) -> Vec<u8> {
    let text = input.text;
    let text_size = text.len() as u32;
    let comment = comment_record(input.version, input.build);

    // External symbols referenced by `.text` relocations (globals, callees), in
    // first-reference order. With none, this is the original leaf layout and the
    // bytes are produced identically to before (no `.rela.text`, no extra symbols).
    let mut externals: Vec<&str> = Vec::new();
    for relocation in &input.relocations {
        if !externals.iter().any(|name| *name == relocation.symbol.as_str()) {
            externals.push(&relocation.symbol);
        }
    }
    let has_text_relocations = !externals.is_empty();

    // Section indices depend on whether `.rela.text` is present (it sits between
    // `.mwcats.text` and `.rela.mwcats.text`, shifting everything after it).
    let section_count: u16 = if has_text_relocations { 9 } else { 8 };
    let symtab_section = if has_text_relocations { 5 } else { 4 };
    let strtab_section = symtab_section + 1;
    let shstrtab_section = strtab_section + 1;

    // .shstrtab — section header names, in section order.
    let mut shstrtab = StringTable::new();
    let name_text = shstrtab.add(".text");
    let name_mwcats = shstrtab.add(".mwcats.text");
    let name_rela_text = if has_text_relocations { shstrtab.add(".rela.text") } else { 0 };
    let name_rela_mwcats = shstrtab.add(".rela.mwcats.text");
    let name_symtab = shstrtab.add(".symtab");
    let name_strtab = shstrtab.add(".strtab");
    let name_shstrtab = shstrtab.add(".shstrtab");
    let name_comment = shstrtab.add(".comment");

    // .strtab — symbol names: source file, then the externals, then the function.
    let mut strtab = StringTable::new();
    let name_source = strtab.add(input.source_name);
    let external_names: Vec<u32> = externals.iter().map(|name| strtab.add(name)).collect();
    let name_function = strtab.add(input.function_name);

    // .symtab — null, FILE, SECTION .text, SECTION .mwcats.text, the undefined
    // externals, then the function. The externals are the first GLOBAL symbols, so
    // they precede the function and `sh_info` (first global) stays at index 4.
    let first_global_index: u32 = 4;
    let function_symbol_index = first_global_index + externals.len() as u32;
    let mut symtab = Vec::new();
    write_symbol(&mut symtab, 0, 0, 0, 0, 0, 0); // null
    write_symbol(&mut symtab, name_source, 0, 0, STT_FILE, 0, SHN_ABS);
    write_symbol(&mut symtab, 0, 0, 0, STT_SECTION, 0, 1); // .text
    write_symbol(&mut symtab, 0, 0, 0, STT_SECTION, 0, 2); // .mwcats.text
    for &name in &external_names {
        write_symbol(&mut symtab, name, 0, 0, STB_GLOBAL_NOTYPE, 0, SHN_UNDEF);
    }
    write_symbol(&mut symtab, name_function, 0, text_size, STB_GLOBAL_FUNC, 0, 1);

    // .mwcats.text — (tag | text size, &function). The second word is relocated.
    let mut mwcats = Vec::new();
    write_u32(&mut mwcats, 0x0200_0000 | text_size);
    write_u32(&mut mwcats, 0);

    // .rela.text — one entry per `.text` relocation, against the external symbol.
    let mut rela_text = Vec::new();
    for relocation in &input.relocations {
        let position = externals.iter().position(|name| *name == relocation.symbol.as_str()).unwrap();
        let symbol = first_global_index + position as u32;
        write_rela(&mut rela_text, relocation.offset, symbol, relocation.elf_type, 0);
    }

    // .rela.mwcats.text — point the trailer word at the function.
    let mut rela_mwcats = Vec::new();
    write_rela(&mut rela_mwcats, 4, function_symbol_index, R_PPC_ADDR32, 0);

    // File offsets. The 4-aligned sections (.text, .mwcats, .rela, .symtab) have
    // 4-aligned sizes and pack contiguously; the string/comment sections are
    // byte-aligned; the section-header table is padded to 8.
    let text_offset = ELF_HEADER_SIZE;
    let mwcats_offset = text_offset + text_size;
    let rela_text_offset = mwcats_offset + mwcats.len() as u32;
    let rela_mwcats_offset = rela_text_offset + rela_text.len() as u32;
    let symtab_offset = rela_mwcats_offset + rela_mwcats.len() as u32;
    let strtab_offset = symtab_offset + symtab.len() as u32;
    let shstrtab_offset = strtab_offset + strtab.bytes.len() as u32;
    let comment_offset = shstrtab_offset + shstrtab.bytes.len() as u32;
    // mwcceppc aligns the section-header table to an 8-byte boundary.
    let section_headers_offset = align8(comment_offset + comment.len() as u32);

    let mut output = Vec::new();
    write_elf_header(&mut output, section_headers_offset, section_count, shstrtab_section as u16);

    // Section payloads, in section order.
    output.extend_from_slice(text);
    output.extend_from_slice(&mwcats);
    if has_text_relocations {
        output.extend_from_slice(&rela_text);
    }
    output.extend_from_slice(&rela_mwcats);
    output.extend_from_slice(&symtab);
    output.extend_from_slice(&strtab.bytes);
    output.extend_from_slice(&shstrtab.bytes);
    output.extend_from_slice(&comment);
    while output.len() < section_headers_offset as usize {
        output.push(0);
    }

    // Section headers.
    let mut header = |name, kind, flags, offset, size, link, info, align, entsize| {
        write_section_header(&mut output, name, kind, flags, offset, size, link, info, align, entsize);
    };
    header(0, 0, 0, 0, 0, 0, 0, 0, 0); // null
    header(name_text, SHT_PROGBITS, SHF_WRITE_EXEC, text_offset, text_size, 0, 0, 4, 0);
    header(name_mwcats, SHT_MWCATS, 0, mwcats_offset, mwcats.len() as u32, 1, 0, 4, 1);
    if has_text_relocations {
        // sh_link -> .symtab, sh_info -> .text (section 1).
        header(name_rela_text, SHT_RELA, 0, rela_text_offset, rela_text.len() as u32, symtab_section, 1, 4, 12);
    }
    // sh_link -> .symtab, sh_info -> .mwcats.text (section 2).
    header(name_rela_mwcats, SHT_RELA, 0, rela_mwcats_offset, rela_mwcats.len() as u32, symtab_section, 2, 4, 12);
    header(name_symtab, SHT_SYMTAB, 0, symtab_offset, symtab.len() as u32, strtab_section, first_global_index, 4, 16);
    // Metrowerks stamps string tables with sh_entsize = 1.
    header(name_strtab, SHT_STRTAB, 0, strtab_offset, strtab.bytes.len() as u32, 0, 0, 1, 1);
    header(name_shstrtab, SHT_STRTAB, 0, shstrtab_offset, shstrtab.bytes.len() as u32, 0, 0, 1, 1);
    header(name_comment, SHT_PROGBITS, 0, comment_offset, comment.len() as u32, 0, 0, 1, 1);

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
