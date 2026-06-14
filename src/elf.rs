//! Minimal ELF32 big-endian PowerPC relocatable-object writer.
//!
//! Emits enough for `powerpc-eabi-objdump -d` to disassemble `.text` and show
//! the function symbol. v0 has no relocations or .data; those arrive with the
//! constant pool. Section layout:
//!   [0] null  [1] .text  [2] .symtab  [3] .strtab  [4] .shstrtab

pub fn write_object(func_name: &str, text: &[u8]) -> Vec<u8> {
    // --- string tables ---
    // .shstrtab
    let mut shstr = vec![0u8];
    let name_off = |s: &mut Vec<u8>, name: &str| -> u32 {
        let off = s.len() as u32;
        s.extend_from_slice(name.as_bytes());
        s.push(0);
        off
    };
    let sh_text = name_off(&mut shstr, ".text");
    let sh_symtab = name_off(&mut shstr, ".symtab");
    let sh_strtab = name_off(&mut shstr, ".strtab");
    let sh_shstrtab = name_off(&mut shstr, ".shstrtab");

    // .strtab (symbol names)
    let mut strtab = vec![0u8];
    let sym_name = name_off(&mut strtab, func_name);

    // .symtab: [0] null, [1] func (global, func, shndx=.text)
    let mut symtab = Vec::new();
    sym(&mut symtab, 0, 0, 0, 0, 0, 0); // null
    let st_info = (1 << 4) | 2; // STB_GLOBAL<<4 | STT_FUNC
    sym(&mut symtab, sym_name, 0, text.len() as u32, st_info, 0, 1);

    // --- file layout ---
    let eh_size = 52u32;
    let sh_entsize = 40u32;
    let num_sh = 5u32;

    let off_text = eh_size;
    let off_symtab = align4(off_text + text.len() as u32);
    let off_strtab = off_symtab + symtab.len() as u32;
    let off_shstr = off_strtab + strtab.len() as u32;
    let off_sh = align4(off_shstr + shstr.len() as u32);

    let mut out = Vec::new();

    // ELF header
    out.extend_from_slice(&[0x7f, b'E', b'L', b'F']);
    out.push(1); // ELFCLASS32
    out.push(2); // ELFDATA2MSB (big-endian)
    out.push(1); // EV_CURRENT
    out.extend_from_slice(&[0u8; 9]); // padding
    be16(&mut out, 1); // e_type = ET_REL
    be16(&mut out, 20); // e_machine = EM_PPC
    be32(&mut out, 1); // e_version
    be32(&mut out, 0); // e_entry
    be32(&mut out, 0); // e_phoff
    be32(&mut out, off_sh); // e_shoff
    be32(&mut out, 0); // e_flags
    be16(&mut out, eh_size as u16); // e_ehsize
    be16(&mut out, 0); // e_phentsize
    be16(&mut out, 0); // e_phnum
    be16(&mut out, sh_entsize as u16); // e_shentsize
    be16(&mut out, num_sh as u16); // e_shnum
    be16(&mut out, 4); // e_shstrndx (.shstrtab)

    // section payloads
    out.extend_from_slice(text);
    pad4(&mut out);
    out.extend_from_slice(&symtab);
    out.extend_from_slice(&strtab);
    out.extend_from_slice(&shstr);
    pad4(&mut out);

    // section headers
    // [0] null
    shdr(&mut out, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    // [1] .text  PROGBITS, ALLOC|EXECINSTR, align 4
    shdr(&mut out, sh_text, 1, 0x6, 0, off_text, text.len() as u32, 0, 0, 4, 0);
    // [2] .symtab  link=strtab(3), info=first-global(1), entsize 16
    shdr(&mut out, sh_symtab, 2, 0, 0, off_symtab, symtab.len() as u32, 3, 1, 4, 16);
    // [3] .strtab
    shdr(&mut out, sh_strtab, 3, 0, 0, off_strtab, strtab.len() as u32, 0, 0, 1, 0);
    // [4] .shstrtab
    shdr(&mut out, sh_shstrtab, 3, 0, 0, off_shstr, shstr.len() as u32, 0, 0, 1, 0);

    out
}

fn align4(x: u32) -> u32 {
    (x + 3) & !3
}
fn pad4(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}
fn be16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}
fn be32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn sym(out: &mut Vec<u8>, name: u32, value: u32, size: u32, info: u8, other: u8, shndx: u16) {
    be32(out, name);
    be32(out, value);
    be32(out, size);
    out.push(info);
    out.push(other);
    be16(out, shndx);
}

#[allow(clippy::too_many_arguments)]
fn shdr(
    out: &mut Vec<u8>,
    name: u32,
    typ: u32,
    flags: u32,
    addr: u32,
    offset: u32,
    size: u32,
    link: u32,
    info: u32,
    addralign: u32,
    entsize: u32,
) {
    be32(out, name);
    be32(out, typ);
    be32(out, flags);
    be32(out, addr);
    be32(out, offset);
    be32(out, size);
    be32(out, link);
    be32(out, info);
    be32(out, addralign);
    be32(out, entsize);
}
