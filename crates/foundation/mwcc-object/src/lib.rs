//! ELF32 big-endian PowerPC relocatable-object writer.
//!
//! We own the object bytes deliberately. Decomp tooling keys on exact section
//! ordering, symbol order, alignment, and the Metrowerks `.comment` record, so
//! the container must be under our control just as the `.text` is. A general
//! object-writer crate (e.g. `object`) becomes worth its weight once we emit
//! relocations and `.data`/`.sdata` (roadmap M3); until then this stays small
//! and exact.
//!
//! Section layout: `[0] null  [1] .text  [2] .symtab  [3] .strtab  [4] .shstrtab`.

/// A defined symbol in the object (currently: a function in `.text`).
pub struct DefinedFunction<'a> {
    pub name: &'a str,
    pub text: &'a [u8],
}

/// Write a relocatable object containing a single function's `.text`.
///
/// v0 emits one function per translation unit; multiple sections, multiple
/// symbols, and relocations follow on the roadmap.
pub fn write_single_function(function: &DefinedFunction<'_>) -> Vec<u8> {
    let text = function.text;

    // .shstrtab (section header names)
    let mut section_names = vec![0u8];
    let name_offset = |table: &mut Vec<u8>, name: &str| -> u32 {
        let offset = table.len() as u32;
        table.extend_from_slice(name.as_bytes());
        table.push(0);
        offset
    };
    let name_text = name_offset(&mut section_names, ".text");
    let name_symtab = name_offset(&mut section_names, ".symtab");
    let name_strtab = name_offset(&mut section_names, ".strtab");
    let name_shstrtab = name_offset(&mut section_names, ".shstrtab");

    // .strtab (symbol names)
    let mut symbol_names = vec![0u8];
    let name_function = name_offset(&mut symbol_names, function.name);

    // .symtab: [0] null, [1] function (global, func, defined in .text)
    let mut symbol_table = Vec::new();
    write_symbol(&mut symbol_table, 0, 0, 0, 0, 0, 0);
    const STB_GLOBAL_STT_FUNC: u8 = (1 << 4) | 2;
    write_symbol(&mut symbol_table, name_function, 0, text.len() as u32, STB_GLOBAL_STT_FUNC, 0, 1);

    // File layout.
    const HEADER_SIZE: u32 = 52;
    const SECTION_HEADER_SIZE: u32 = 40;
    const SECTION_COUNT: u32 = 5;

    let text_offset = HEADER_SIZE;
    let symtab_offset = align4(text_offset + text.len() as u32);
    let strtab_offset = symtab_offset + symbol_table.len() as u32;
    let shstrtab_offset = strtab_offset + symbol_names.len() as u32;
    let section_headers_offset = align4(shstrtab_offset + section_names.len() as u32);

    let mut output = Vec::new();

    // ELF header.
    output.extend_from_slice(&[0x7f, b'E', b'L', b'F']);
    output.push(1); // ELFCLASS32
    output.push(2); // ELFDATA2MSB (big-endian)
    output.push(1); // EV_CURRENT
    output.extend_from_slice(&[0u8; 9]); // e_ident padding
    write_u16(&mut output, 1); // e_type = ET_REL
    write_u16(&mut output, 20); // e_machine = EM_PPC
    write_u32(&mut output, 1); // e_version
    write_u32(&mut output, 0); // e_entry
    write_u32(&mut output, 0); // e_phoff
    write_u32(&mut output, section_headers_offset); // e_shoff
    write_u32(&mut output, 0); // e_flags
    write_u16(&mut output, HEADER_SIZE as u16); // e_ehsize
    write_u16(&mut output, 0); // e_phentsize
    write_u16(&mut output, 0); // e_phnum
    write_u16(&mut output, SECTION_HEADER_SIZE as u16); // e_shentsize
    write_u16(&mut output, SECTION_COUNT as u16); // e_shnum
    write_u16(&mut output, 4); // e_shstrndx (.shstrtab)

    // Section payloads.
    output.extend_from_slice(text);
    pad4(&mut output);
    output.extend_from_slice(&symbol_table);
    output.extend_from_slice(&symbol_names);
    output.extend_from_slice(&section_names);
    pad4(&mut output);

    // Section headers.
    write_section_header(&mut output, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0); // null
    // .text: PROGBITS, ALLOC | EXECINSTR, 4-byte aligned.
    write_section_header(&mut output, name_text, 1, 0x6, 0, text_offset, text.len() as u32, 0, 0, 4, 0);
    // .symtab: link -> .strtab (3), info -> first global symbol index (1), entsize 16.
    write_section_header(&mut output, name_symtab, 2, 0, 0, symtab_offset, symbol_table.len() as u32, 3, 1, 4, 16);
    // .strtab
    write_section_header(&mut output, name_strtab, 3, 0, 0, strtab_offset, symbol_names.len() as u32, 0, 0, 1, 0);
    // .shstrtab
    write_section_header(&mut output, name_shstrtab, 3, 0, 0, shstrtab_offset, section_names.len() as u32, 0, 0, 1, 0);

    output
}

fn align4(value: u32) -> u32 {
    (value + 3) & !3
}
fn pad4(output: &mut Vec<u8>) {
    while output.len() % 4 != 0 {
        output.push(0);
    }
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

#[allow(clippy::too_many_arguments)]
fn write_section_header(
    output: &mut Vec<u8>,
    name: u32,
    section_type: u32,
    flags: u32,
    address: u32,
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
    write_u32(output, address);
    write_u32(output, offset);
    write_u32(output, size);
    write_u32(output, link);
    write_u32(output, info);
    write_u32(output, alignment);
    write_u32(output, entry_size);
}
