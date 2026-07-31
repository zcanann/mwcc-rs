fn be_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn be_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn c_string(bytes: &[u8], offset: usize) -> &str {
    let length = bytes[offset..]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap();
    std::str::from_utf8(&bytes[offset..offset + length]).unwrap()
}

struct SymbolRecord {
    name: String,
    section: String,
    section_offset: usize,
    value: u32,
    size: u32,
    binding: u8,
}

fn symbol_records(object: &[u8]) -> Vec<SymbolRecord> {
    let section_headers = be_u32(object, 32) as usize;
    let section_size = be_u16(object, 46) as usize;
    let section_count = be_u16(object, 48) as usize;
    let section_name_index = be_u16(object, 50) as usize;
    let section_header = |index: usize| section_headers + index * section_size;
    let section_names_offset =
        be_u32(object, section_header(section_name_index) + 16) as usize;
    let section_names = (0..section_count)
        .map(|index| {
            let name = be_u32(object, section_header(index)) as usize;
            c_string(object, section_names_offset + name).to_owned()
        })
        .collect::<Vec<_>>();
    let symtab_index = (0..section_count)
        .find(|index| be_u32(object, section_header(*index) + 4) == 2)
        .unwrap();
    let symtab = section_header(symtab_index);
    let symtab_offset = be_u32(object, symtab + 16) as usize;
    let symtab_size = be_u32(object, symtab + 20) as usize;
    let strings_index = be_u32(object, symtab + 24) as usize;
    let strings_offset = be_u32(object, section_header(strings_index) + 16) as usize;

    (0..symtab_size / 16)
        .filter_map(|index| {
            let symbol = symtab_offset + index * 16;
            let name = c_string(
                object,
                strings_offset + be_u32(object, symbol) as usize,
            );
            let section = be_u16(object, symbol + 14) as usize;
            (section < section_names.len()).then(|| SymbolRecord {
                name: name.to_owned(),
                section: section_names[section].clone(),
                section_offset: be_u32(object, section_header(section) + 16) as usize,
                value: be_u32(object, symbol + 4),
                size: be_u32(object, symbol + 8),
                binding: object[symbol + 12] >> 4,
            })
        })
        .collect()
}

pub(super) fn symbols(object: &[u8]) -> Vec<(String, String, u32, u8)> {
    symbol_records(object)
        .into_iter()
        .map(|symbol| {
            (
                symbol.name,
                symbol.section,
                symbol.value,
                symbol.binding,
            )
        })
        .collect()
}

pub(super) fn function_bytes<'a>(object: &'a [u8], name: &str) -> &'a [u8] {
    let symbol = symbol_records(object)
        .into_iter()
        .find(|symbol| symbol.name == name && symbol.section == ".text")
        .unwrap_or_else(|| panic!("missing .text symbol {name}"));
    let start = symbol.section_offset + symbol.value as usize;
    &object[start..start + symbol.size as usize]
}
