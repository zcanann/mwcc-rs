//! Assembly of a relocatable object, byte-for-byte as mwcceppc emits it. The
//! object holds one or more functions sharing a single `.text`; the Metrowerks
//! `.mwcats.text` section carries one `(0x02000000 | function size, &function)`
//! record per function, each with its relocation, alongside a symbol table (file,
//! section, the anonymous `@N` locals, the undefined externals, and a symbol per
//! function) and the `.comment` record.
//!
//! Float/stack-frame functions add `.sdata2` / `extab` / `extabindex` sections,
//! pooled across all functions in the unit.

use crate::{CommentFormat, DataObject, ObjectInput, RelocationTarget};
use std::collections::HashMap;

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
const STB_WEAK_FUNC: u8 = (2 << 4) | 2; // STB_WEAK | STT_FUNC (__declspec(weak))
const STB_WEAK_OBJECT: u8 = (2 << 4) | 1; // STB_WEAK | STT_OBJECT (an inline's static local)
const STB_LOCAL_FUNC: u8 = 2; // STB_LOCAL | STT_FUNC (a `static` function)
/// A `.comment` per-symbol attribute set by `#pragma force_active on` — stamped on
/// the function symbol and its inline-`asm` `entry` symbols (animal_crossing runtime.c).
const FORCE_ACTIVE_FLAG: u32 = 0x0008_0000;
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

/// The `.comment` record for a specific compiler version and build, plus one
/// (alignment, flags) pair per symbol past null and FILE, in order. The true
/// framing is a 60-byte prefix then `[align u32][flags u32]` per symbol —
/// byte-identical to the old pad+align reading when every flag is 0; a WEAK
/// function carries flags 0x0e000000 (measured: weak-first, weak-last, and
/// weak-only orderings all place the 0x0e word right after the weak align).
fn comment_record(format: CommentFormat, symbol_records: &[(u32, u32)]) -> Vec<u8> {
    let mut record = COMMENT_PREFIX.to_vec();
    record[11] = format.marker;
    record[12] = format.version.0;
    record[13] = format.version.1;
    record[14] = format.version.2;
    record.extend_from_slice(&[0, 0, 0, 0]);
    for &(alignment, flags) in symbol_records {
        record.extend_from_slice(&alignment.to_be_bytes());
        record.extend_from_slice(&flags.to_be_bytes());
    }
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
    // `.text` LAYOUT order: a text_deferred function (a materialized static
    // inline) lays out AFTER the next non-deferred function — mwcc's deferred
    // materialization queue (measured: ww alloc's dealloc_var after
    // dealloc_fixed, __pool_free after free). Symbols keep source position.
    let layout_order: Vec<usize> = {
        let mut order = Vec::with_capacity(functions.len());
        let mut pending: Vec<usize> = Vec::new();
        for (index, function) in functions.iter().enumerate() {
            if function.text_deferred {
                pending.push(index);
            } else {
                order.push(index);
                order.append(&mut pending);
            }
        }
        order.append(&mut pending);
        order
    };
    let mut function_offset: Vec<u32> = vec![0; functions.len()];
    let mut function_size: Vec<u32> = vec![0; functions.len()];
    for &index in &layout_order {
        function_offset[index] = text.len() as u32;
        function_size[index] = functions[index].text.len() as u32;
        text.extend_from_slice(functions[index].text);
    }

    let has_text_relocations = functions
        .iter()
        .any(|function| !function.relocations.is_empty());
    let has_frame = functions.iter().any(|function| function.frame.is_some());
    let has_constants = functions
        .iter()
        .any(|function| !function.constants.is_empty());
    // Each defined object is routed to a section by const-ness, size, and whether
    // it is initialized: a writable global to `.sdata` (initialized) or `.sbss`
    // (zero), a const one to `.sdata2` (≤ 8 bytes) or `.rodata` (larger). mwcc lays
    // `.sbss` (small zero) out in REVERSE declaration order; every other data
    // section — including `.bss` (large zero) — is FORWARD. (Verified against the
    // real compiler: two small uninitialized scalars reverse, two large ones don't.)
    // Const objects are read-only (`.sdata2`/`.rodata`); writable ones split by the
    // 8-byte small-data threshold: small to `.sdata`/`.sbss`, large to `.data`/`.bss`.
    let section_of = |object: &DataObject| -> &'static str {
        // An explicit `__declspec(section "…")` override wins over the default
        // routing. Only the sections the writer knows how to emit are honored.
        if let Some(section) = object.section {
            return match section {
                ".ctors" => ".ctors",
                ".dtors" => ".dtors",
                _ => ".data",
            };
        }
        if object.is_const {
            if object.size <= 8 && !object.force_full_data_section {
                ".sdata2"
            } else {
                ".rodata"
            }
        } else if object.size <= 8 && !object.force_full_data_section {
            if object.initial_bytes.is_some() {
                ".sdata"
            } else {
                ".sbss"
            }
        } else if object.initial_bytes.is_some() {
            ".data"
        } else {
            ".bss"
        }
    };
    let has_sdata = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".sdata");
    let has_sbss = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".sbss");
    let has_rodata = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".rodata")
        || input
            .functions
            .iter()
            .any(|function| !function.anonymous_rodata.is_empty());
    let has_const_sdata2 = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".sdata2");
    let has_file_data = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".data");
    let has_bss = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".bss");
    let has_ctors = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".ctors");
    let has_dtors = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".dtors");

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
    // `.ctors`/`.dtors` (`__declspec(section "…")` constructor/destructor-chain
    // references) sit right after `.text` — `.ctors` first (measured) — in forward
    // declaration order: four zero bytes per entry, each patched by an `ADDR32`
    // relocation to its function.
    let mut ctors_size = 0u32;
    for object in input
        .data_objects
        .iter()
        .filter(|object| section_of(object) == ".ctors")
    {
        place(object, ".ctors", &mut ctors_size);
    }
    let mut dtors_size = 0u32;
    for object in input
        .data_objects
        .iter()
        .filter(|object| section_of(object) == ".dtors")
    {
        place(object, ".dtors", &mut dtors_size);
    }
    // The const `.sdata2` globals occupy the FRONT of the constant pool (ahead of
    // any function float constants), in forward declaration order.
    let mut sdata2_global_size = 0u32;
    for object in input
        .data_objects
        .iter()
        .filter(|object| section_of(object) == ".sdata2")
    {
        place(object, ".sdata2", &mut sdata2_global_size);
    }
    let mut rodata_size = 0u32;
    for object in input
        .data_objects
        .iter()
        .filter(|object| section_of(object) == ".rodata")
    {
        place(object, ".rodata", &mut rodata_size);
    }
    // Anonymous `.rodata` blobs follow the named const objects, 4-aligned
    // (measured on strtold: "INFINITY" at 0x2c right after the 42-byte
    // template, the 32-byte template at 0x38).
    let mut rodata_blob_offset: Vec<Vec<u32>> = Vec::new();
    for function in &input.functions {
        let mut offsets = Vec::new();
        for (bytes, _) in &function.anonymous_rodata {
            rodata_size = rodata_size.div_ceil(4) * 4;
            offsets.push(rodata_size);
            rodata_size += bytes.len() as u32;
        }
        rodata_blob_offset.push(offsets);
    }
    // Large initialized `.data` is laid out in CREATION order: file-scope
    // objects (declaration order) first, then per function its pooled `.data`
    // strings followed by its jump table (measured: ww's table at 0x80 sits
    // between two_exp's long strings and dummy's later string at 0x1a4).
    let string_owner: std::collections::HashMap<&str, usize> = input
        .functions
        .iter()
        .enumerate()
        .flat_map(|(function_index, function)| {
            function
                .string_names
                .iter()
                .map(move |name| (name.as_str(), function_index))
        })
        .collect();
    let mut file_data_size = 0u32;
    for object in input.data_objects.iter().filter(|object| {
        section_of(object) == ".data"
            && !string_owner.contains_key(object.name)
            && object.static_local_owner.is_none()
    }) {
        place(object, ".data", &mut file_data_size);
    }
    let mut jump_table_offset: Vec<Vec<Option<u32>>> = input
        .functions
        .iter()
        .map(|function| vec![None; function.jump_tables.len()])
        .collect();
    for (function_index, function) in input.functions.iter().enumerate() {
        // The function's own `.data` STATIC LOCALS lead its block (declared at
        // the body top, before any string literal use — bfbb's pow_10$1224 at
        // 0x1a8 ahead of dec2num's pooled string).
        for object in input.data_objects.iter().filter(|object| {
            object.static_local_owner == Some(function_index) && section_of(object) == ".data"
        }) {
            place(object, ".data", &mut file_data_size);
        }
        for name in &function.string_names {
            if let Some(object) = input
                .data_objects
                .iter()
                .find(|object| object.name == name.as_str() && section_of(object) == ".data")
            {
                place(object, ".data", &mut file_data_size);
            }
        }
        for (table_index, table) in function.jump_tables.iter().enumerate().rev() {
            file_data_size = file_data_size.div_ceil(4) * 4;
            jump_table_offset[function_index][table_index] = Some(file_data_size);
            file_data_size += table.entries.len() as u32 * 4;
        }
    }
    let has_jump_table = input
        .functions
        .iter()
        .any(|function| !function.jump_tables.is_empty());
    // Large zero `.bss` lays out in SYMBOL-EMISSION order, not declaration order:
    // referenced objects first (in the order functions reference them — their
    // `symbol_order`, across functions in source order), then any unreferenced ones
    // in REVERSE declaration order. (This matches both mwcc's `.bss` offsets and the
    // symbol table. The small-data `.sbss` instead reverses unconditionally, below.)
    let mut bss_size = 0u32;
    let mut placed_bss: std::collections::HashSet<&'a str> = std::collections::HashSet::new();
    for function in &input.functions {
        // A capture-emitted function has an EMPTY symbol_order — its `.text`
        // relocations carry the reference order instead (measured: wind_waker
        // abort_exit, whose __atexit_funcs places by first reference).
        let relocation_names =
            function
                .relocations
                .iter()
                .filter_map(|relocation| match &relocation.target {
                    RelocationTarget::External(name)
                    | RelocationTarget::ExternalWithAddend(name, _) => Some(name.as_str()),
                    _ => None,
                });
        for name in function
            .symbol_order
            .iter()
            .map(|name| name.as_str())
            .chain(relocation_names)
        {
            if let Some(object) = input
                .data_objects
                .iter()
                .find(|object| object.name == name && section_of(object) == ".bss")
            {
                if placed_bss.insert(object.name) {
                    place(object, ".bss", &mut bss_size);
                }
            }
        }
    }
    for object in input
        .data_objects
        .iter()
        .rev()
        .filter(|object| section_of(object) == ".bss")
    {
        if placed_bss.insert(object.name) {
            place(object, ".bss", &mut bss_size);
        }
    }
    let mut sdata_size = 0u32;
    for object in input
        .data_objects
        .iter()
        .filter(|object| section_of(object) == ".sdata")
    {
        place(object, ".sdata", &mut sdata_size);
    }
    // mwcc lays `.sbss` out as: EXPLICITLY zero-initialized globals (`int a = 0;`) in
    // DECLARATION order, then UNINITIALIZED globals (`int a;`) in REVERSE declaration order.
    // (An all-uninitialized `.sbss` therefore just reverses, as before.)
    let mut sbss_size = 0u32;
    if input.object_format.small_zero_statics_in_declaration_order {
        for object in input.data_objects.iter().filter(|object| {
            section_of(object) == ".sbss" && !object.is_static && object.is_explicit_zero
        }) {
            place(object, ".sbss", &mut sbss_size);
        }
        for object in input
            .data_objects
            .iter()
            .filter(|object| section_of(object) == ".sbss" && object.is_static)
        {
            place(object, ".sbss", &mut sbss_size);
        }
        for object in input.data_objects.iter().rev().filter(|object| {
            section_of(object) == ".sbss" && !object.is_static && !object.is_explicit_zero
        }) {
            place(object, ".sbss", &mut sbss_size);
        }
    } else {
        for object in input
            .data_objects
            .iter()
            .filter(|object| section_of(object) == ".sbss" && object.is_explicit_zero)
        {
            place(object, ".sbss", &mut sbss_size);
        }
        for object in input
            .data_objects
            .iter()
            .rev()
            .filter(|object| section_of(object) == ".sbss" && !object.is_explicit_zero)
        {
            place(object, ".sbss", &mut sbss_size);
        }
    }
    // `.sdata`/`.rodata`/`.data` file bytes: each initialized object's bytes at its
    // offset. (`.sdata2` const-global bytes are laid into the pool below, with the
    // floats; `.bss` is zero-initialized so has no file bytes.)
    let mut sdata = vec![0u8; sdata_size as usize];
    let mut rodata = vec![0u8; rodata_size as usize];
    let mut file_data = vec![0u8; file_data_size as usize];
    let mut ctors = vec![0u8; ctors_size as usize];
    let mut dtors = vec![0u8; dtors_size as usize];
    for object in &input.data_objects {
        if let Some(bytes) = &object.initial_bytes {
            let offset = data_offsets[object.name] as usize;
            match section_of(object) {
                ".sdata" => sdata[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".rodata" => rodata[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".data" => file_data[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".ctors" => ctors[offset..offset + bytes.len()].copy_from_slice(bytes),
                ".dtors" => dtors[offset..offset + bytes.len()].copy_from_slice(bytes),
                _ => {}
            }
        }
    }
    for (function_index, function) in input.functions.iter().enumerate() {
        for (blob_index, (bytes, _)) in function.anonymous_rodata.iter().enumerate() {
            let offset = rodata_blob_offset[function_index][blob_index] as usize;
            rodata[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
    }

    // Jump tables (dense switches) live in a `.data` section: one 4-byte entry per
    // index, every entry filled by an `ADDR32` relocation (so the file bytes are
    // zero). Each table is recorded at its offset; `.data` is 8-aligned.

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
    // mwcc pools each distinct read-only constant once per object: a value a later
    // function reuses (e.g. several functions comparing against `0.0f`) shares the
    // first function's `.sdata2` slot — and, below, its `@N` symbol — rather than
    // appending a duplicate. Dedup by (bits, width); a single and a double zero stay
    // distinct (different widths → different `lfs`/`lfd` slots).
    let mut constant_offsets: Vec<Vec<u32>> = Vec::new();
    let mut pooled_offset: HashMap<(u64, u8), u32> = HashMap::new();
    for function in functions {
        let mut offsets = Vec::new();
        for constant in &function.constants {
            let fresh_slot = |sdata2: &mut Vec<u8>| {
                // An 8-byte STATIC-SLOT entry is a struct IMAGE (two floats/ints):
                // it aligns 4, unlike a genuine double constant (align 8).
                let alignment = if constant.byte_width == 8 && constant.static_slot {
                    4
                } else {
                    constant.byte_width as usize
                };
                while sdata2.len() % alignment != 0 {
                    sdata2.push(0);
                }
                let offset = sdata2.len() as u32;
                match constant.byte_width {
                    8 => sdata2.extend_from_slice(&constant.bits.to_be_bytes()),
                    _ => sdata2.extend_from_slice(&(constant.bits as u32).to_be_bytes()),
                }
                offset
            };
            // A FORCE-NEW slot never joins the dedup map: it takes a fresh
            // offset and leaves the first slot as the shared one.
            let offset = if constant.force_new {
                fresh_slot(&mut sdata2)
            } else {
                *pooled_offset
                    .entry((constant.bits, constant.byte_width))
                    .or_insert_with(|| fresh_slot(&mut sdata2))
            };
            offsets.push(offset);
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
    let mut jump_table_numbers: Vec<Vec<u32>> = Vec::new();
    let mut rodata_blob_numbers: Vec<Vec<u32>> = Vec::new();
    let mut extab_payload_offset = 0u32;
    let mut extabindex_payload_offset = 0u32;
    // The functions' anonymous `@N` numbering starts at 5, past any FILE-SCOPE anonymous objects
    // (their strings/jump tables). A function's own strings are numbered PER-FUNCTION at the front of
    // its `@N` block (below), so they are excluded from this base: `pooled - per-function strings` is
    // the file-scope remainder, and each function then advances by `string_count` before its
    // constants. (A non-string unit has `string_count = 0` throughout, so this is unchanged.)
    let function_string_names: std::collections::HashSet<&str> = functions
        .iter()
        .flat_map(|function| function.string_names.iter().map(String::as_str))
        .collect();
    let pooled_string_count = input
        .data_objects
        .iter()
        .filter(|object| object.is_static && object.name.starts_with('@'))
        .count() as u32;
    let function_string_total: u32 = functions.iter().map(|function| function.string_count).sum();
    // A FILE-SCOPE pooled string declared BETWEEN functions (`static const
    // char* const p = "…"` mid-file — ansi_fp's strikers revision) numbers
    // IN-STREAM at its source position, not up front: it consumes one number
    // right before the next function's block (the resolver assigned its name
    // the same way).
    let is_in_stream_file_string = |object: &DataObject| {
        object.is_static
            && object.name.starts_with('@')
            && object.static_local_owner.is_none()
            && object.functions_before > 0
            && !function_string_names.contains(object.name)
    };
    let in_stream_file_strings = input
        .data_objects
        .iter()
        .filter(|object| is_in_stream_file_string(object))
        .count() as u32;
    let mut counter = u32::from(input.object_format.initial_anonymous_counter)
        + pooled_string_count
        - function_string_total
        - in_stream_file_strings;
    // The `@N` of a pooled constant a later function reuses is the one the first
    // function got — a deduped reuse consumes no new number, so the reusing
    // function's subsequent unwind `@N` shift down accordingly.
    let mut numbered_constant: HashMap<(u64, u8), u32> = HashMap::new();
    // STATIC-SLOT pooled images (auto-array word images at `counter - 1`) dedup
    // across functions like ordinary pool constants.
    let mut numbered_static_slot: HashMap<(u64, u8), u32> = HashMap::new();
    // Real functions' STATIC LOCALS: numbered at counter-1+i (measured: the
    // first function's static is $4 against the base-5 counter), and the
    // owner's constants shift by the static count.
    let mut static_local_numbers: HashMap<&str, u32> = HashMap::new();
    for (function_index, function) in functions.iter().enumerate() {
        // In-stream file strings declared right before this function consume
        // their numbers here.
        counter += input
            .data_objects
            .iter()
            .filter(|object| {
                is_in_stream_file_string(object) && object.functions_before == function_index
            })
            .count() as u32;
        let owned_statics: Vec<&DataObject> = input
            .data_objects
            .iter()
            .filter(|object| object.static_local_owner == Some(function_index))
            .collect();
        for (offset_index, object) in owned_statics.iter().enumerate() {
            // A static numbers at the counter AS OF ITS DECLARATION POINT
            // (fire 494, measured: mp4 uart's initialized$4 declares inside the
            // FIRST skipped inline — no bump has landed; pikmin's same static
            // is $34 behind 30 counts of earlier header inlines; the probe
            // matrix's s$4/s$7). `anonymous_adjust` carries that position.
            static_local_numbers.insert(
                object.name,
                (counter as i64 - 1 + object.anonymous_adjust + offset_index as i64) as u32,
            );
        }
        let mut number = counter + owned_statics.len() as u32 + function.anonymous_bump;
        // This function's own strings sit at the front of its `@N` block, before
        // its constants — unless string_number_after_constants places them
        // after the first K constants (creation order — bfbb's __dec2num).
        if function.string_number_after_constants.is_none()
            && function.string_number_after_rodata.is_none()
        {
            number += function.string_count;
        }
        // The anonymous rodata blob numbers BEFORE the pool constants
        // (measured: __strtold's table @26 precedes its pool double @147).
        {
            let mut numbers_of_blobs = Vec::new();
            for (blob_index, (_, anonymous_offset)) in function.anonymous_rodata.iter().enumerate()
            {
                // string_number_after_rodata: the strings (and a gap before
                // them) number between blob K-1 and blob K (strtold's "NAN("
                // @53 between "INFINITY" @39 and the template @54).
                if let Some((position, gap)) = function.string_number_after_rodata {
                    if position == blob_index as u32 {
                        number += gap + function.string_count;
                    }
                }
                let number_of_blob = (number as i64 + *anonymous_offset as i64) as u32;
                number = number_of_blob + 1;
                numbers_of_blobs.push(number_of_blob);
            }
            if let Some((position, gap)) = function.string_number_after_rodata {
                if position as usize >= function.anonymous_rodata.len() {
                    number += gap + function.string_count;
                }
            }
            rodata_blob_numbers.push(numbers_of_blobs);
        }
        let mut numbers = Vec::new();
        let mut static_slot_seen = 0u32;
        for (constant_index, constant) in function.constants.iter().enumerate() {
            // An initialized auto array's pooled WORD IMAGE numbers at the
            // function's STATIC-LOCAL slot (`counter - 1`, past any owned
            // statics), outside the pool block and consuming no pool number
            // (measured: mbstring's first_byte_mark -> @4). A LATER function
            // reusing the image binds the first one's number (wcstombs -> @4).
            if constant.static_slot {
                match numbered_static_slot.get(&(constant.bits, constant.byte_width)) {
                    Some(&existing) => numbers.push(existing),
                    None => {
                        let number_of_slot =
                            counter + owned_statics.len() as u32 + static_slot_seen - 1;
                        numbered_static_slot
                            .insert((constant.bits, constant.byte_width), number_of_slot);
                        numbers.push(number_of_slot);
                        static_slot_seen += 1;
                    }
                }
                continue;
            }
            if function.string_number_after_constants == Some(constant_index as u32) {
                number += function.string_count;
            }
            for (gap_index, gap) in &function.constant_number_gaps {
                if *gap_index == constant_index {
                    number += gap;
                }
            }
            match numbered_constant.get(&(constant.bits, constant.byte_width)) {
                Some(&existing) if !constant.force_new => numbers.push(existing),
                _ => {
                    if !constant.force_new {
                        numbered_constant.insert((constant.bits, constant.byte_width), number);
                    }
                    numbers.push(number);
                    number += 1;
                }
            }
        }
        if let Some(position) = function.string_number_after_constants {
            if position as usize >= function.constants.len() {
                number += function.string_count;
            }
        }
        constant_numbers.push(numbers);
        // A dense switch's jump table is numbered after the function's internal
        // labels (a label per case, the dispatch, and an explicit `default:`).
        let mut numbers_of_tables = Vec::new();
        for table in &function.jump_tables {
            let number_of_table = number + table.anonymous_offset;
            number = number_of_table;
            numbers_of_tables.push(number_of_table);
        }
        jump_table_numbers.push(numbers_of_tables);
        number += function.post_constant_bump;
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
        let post_function_bump = function.post_function_anonymous_bump.unwrap_or_else(|| {
            if function.frame.is_some() {
                input.object_format.post_framed_function_anonymous_bump
            } else {
                input.object_format.post_leaf_function_anonymous_bump
            }
        });
        counter = number + u32::from(post_function_bump);
    }

    // 1. The ordered section-name list (index 0 is the implicit NULL section). The
    //    unwind tables sit right after `.text`, then the `.sdata2` constant pool;
    //    their `.rela` and everything downstream key off this order, by name. A
    //    data-only unit (no functions) omits `.text` and the `.mwcats` machinery.
    let has_functions = !functions.is_empty();
    // mwcc catalogs only COMPILER-GENERATED functions in `.mwcats.text`; hand-written
    // inline-`asm` functions are excluded. An object whose only functions are asm has
    // no `.mwcats.text`/`.rela.mwcats.text` at all (its code still lives in `.text`).
    let has_mwcats = functions.iter().any(|function| !function.is_asm);
    // A jump table and large writable globals share `.data`; the lowering guarantees
    // they do not co-occur, so one `.data` entry covers both.
    let has_data = has_jump_table || has_file_data;
    // A `__declspec(section "…")` on the functions relocates the code section — and
    // its `.mwcats` catalog and `.rela` sections — from `.text` to that name (the
    // runtime's `__mem.c` uses `.init`). A TU's functions share one code section
    // (mixed `.text`/`.init` is not modeled); the derived names follow the same
    // `.mwcats<sec>` / `.rela<sec>` shape mwcc uses.
    let text_section: &str = input
        .functions
        .iter()
        .find_map(|function| function.section)
        .unwrap_or(".text");
    let mwcats_section: String = format!(".mwcats{text_section}");
    let rela_text_section: String = format!(".rela{text_section}");
    let rela_mwcats_section: String = format!(".rela{mwcats_section}");
    let mut order: Vec<&str> = Vec::new();
    if has_functions {
        order.push(text_section);
    }
    if has_frame {
        order.push("extab");
        order.push("extabindex");
    }
    // `.ctors` then `.dtors` (constructor/destructor-chain references) sit
    // immediately after the text and unwind sections, ahead of the ordinary data
    // sections (measured: .text, .ctors, .dtors, .sdata).
    if has_ctors {
        order.push(".ctors");
    }
    if has_dtors {
        order.push(".dtors");
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
    if has_mwcats {
        order.push(&mwcats_section);
    }
    if has_text_relocations {
        order.push(&rela_text_section);
    }
    if has_frame {
        order.push(".relaextabindex");
    }
    // The `.rela.*` sections follow their target sections' order, so `.rela.sdata`
    // (→ `.sdata`) precedes `.rela.mwcats.text` (→ `.mwcats.text`, last).
    let has_data_relocs = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".data" && !object.relocations.is_empty());
    if has_jump_table || has_data_relocs {
        order.push(".rela.data");
    }
    let has_sdata_relocs = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".sdata" && !object.relocations.is_empty());
    if has_sdata_relocs {
        order.push(".rela.sdata");
    }
    // `.rela.ctors`/`.rela.dtors` follow their target sections' relative order —
    // after `.rela.text` and the data relas, before `.rela.mwcats.text` (measured).
    let has_ctors_relocs = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".ctors" && !object.relocations.is_empty());
    if has_ctors_relocs {
        order.push(".rela.ctors");
    }
    let has_dtors_relocs = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".dtors" && !object.relocations.is_empty());
    if has_dtors_relocs {
        order.push(".rela.dtors");
    }
    // `.rela.sdata2` — a `static const` pointer-to-symbol global in the read-only pool
    // carries an ADDR32 (the global-destructor reference when __declspec is macro'd off).
    // `.sdata2` is the last data section, so its rela precedes only `.rela.mwcats`.
    let has_sdata2_relocs = input
        .data_objects
        .iter()
        .any(|object| section_of(object) == ".sdata2" && !object.relocations.is_empty());
    if has_sdata2_relocs {
        order.push(".rela.sdata2");
    }
    if has_mwcats {
        order.push(&rela_mwcats_section);
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
    let content_sections: Vec<&str> = [
        text_section,
        "extab",
        "extabindex",
        ".ctors",
        ".dtors",
        ".rodata",
        ".data",
        ".bss",
        ".sdata",
        ".sbss",
        ".sdata2",
        &mwcats_section,
    ]
    .into_iter()
    .filter(|name| order.contains(name))
    .collect();
    // The `.comment` trailer carries one record per symbol *after* the null and
    // FILE entries, holding that symbol's alignment (0 for an undefined external).
    // Values are collected here in symbol-emission order.
    let mut comment_values: Vec<(u32, u32)> = Vec::new();
    // mwcc raises a data section's alignment (sh_addralign) to the MAX alignment of the
    // objects it holds: a `__attribute__((aligned(32)))` global makes its `.bss`/`.sdata`
    // 32-aligned rather than the 8-byte default (dolphin DMA buffers). Compute the per-
    // section max object alignment (data_aligns/data_section are fully populated by now).
    let mut section_max_align: std::collections::HashMap<&str, u32> =
        std::collections::HashMap::new();
    for (name, section) in &data_section {
        let entry = section_max_align.entry(*section).or_insert(0);
        *entry = (*entry).max(data_aligns[name]);
    }
    let section_align = |name: &str| -> u32 {
        let base = match name {
            ".sdata2" | ".sdata" | ".sbss" | ".data" | ".bss" | ".rodata" => 8,
            _ => 4,
        };
        base.max(section_max_align.get(name).copied().unwrap_or(0))
    };
    let mut strtab = StringTable::new();
    let mut symtab = Vec::new();
    write_symbol(&mut symtab, 0, 0, 0, 0, 0, 0); // null
    write_symbol(
        &mut symtab,
        strtab.add(input.source_name),
        0,
        0,
        STT_FILE,
        0,
        SHN_ABS,
    );
    for name in &content_sections {
        write_symbol(&mut symtab, 0, 0, 0, STT_SECTION, 0, index_of(name) as u16);
        comment_values.push((section_align(name), 0));
    }
    // `static inline` asm helpers (e.g. OSFastCast.h) — a local undefined symbol
    // each, in declaration order, right after the section symbols. `info = 0` is
    // STB_LOCAL | STT_NOTYPE; an undefined symbol has `.comment` alignment 0.
    // A helper a function actually CALLS (a capture's bl — pikmin s_ldexp's
    // __fpclassifyd__Fd) instead emits inside that function's referenced-extern
    // run, at its reference position, still LOCAL (measured: after the pool
    // constants, before copysign).
    let referenced_inline_asm: std::collections::HashSet<&str> = input
        .functions
        .iter()
        .flat_map(|function| function.relocations.iter())
        .filter_map(|relocation| match &relocation.target {
            RelocationTarget::External(name) | RelocationTarget::ExternalWithAddend(name, _) => {
                Some(name.as_str())
            }
            _ => None,
        })
        .filter(|name| input.inline_asm_symbols.iter().any(|symbol| symbol == name))
        .collect();
    for name in input.inline_asm_symbols {
        if referenced_inline_asm.contains(name.as_str()) {
            continue;
        }
        write_symbol(&mut symtab, strtab.add(name), 0, 0, 0, 0, SHN_UNDEF);
        comment_values.push((0, 0));
    }
    // `static` (file-local) data objects: a LOCAL object symbol each, after the
    // inline-asm locals and before the functions' `@N` entries. The INITIALIZED ones
    // (`.sdata`/`.data`) come first in FORWARD declaration order, then the ZERO ones
    // (`.sbss`/`.bss`) in REVERSE — the same split (and same order) the uninitialized
    // globals follow. A static declared AFTER a function (`functions_before > 0`) is
    // SKIPPED here and emitted at its source position in the per-function run below
    // (both const and non-const — byte-verified). Their indices are kept so a function
    // relocation that targets one resolves locally.
    let mut local_data_symbols: std::collections::HashMap<&str, u32> =
        std::collections::HashMap::new();
    let mut emitted_zero_static: std::collections::HashSet<&str> = std::collections::HashSet::new();
    // A function-body string's `@N` data object carries its bytes here (for section layout) but its
    // SYMBOL is emitted per-function in the `@N` run below, interleaved with that function's
    // constants/unwind entries — so skip those objects in this grouped static run. (A FILE-SCOPE
    // string — `char *s = "…";` — is not in any function's `string_names`, so it stays here, numbered
    // ahead of the functions.)
    let is_zero_section = |name: &str| matches!(data_section[name], ".sbss" | ".bss");
    // Initialized statics — plus EXPLICITLY zero-initialized small `.sbss` ones — first, in
    // FORWARD declaration order (same interleaving mwcc uses for exported globals).
    let static_forward = |object: &DataObject| {
        !is_zero_section(object.name)
            || (data_section[object.name] == ".sbss" && object.is_explicit_zero)
    };
    let is_pending_zero_static = |object: &DataObject| {
        object.is_static
            && is_zero_section(object.name)
            && !static_forward(object)
            && object.static_local_owner.is_none()
    };
    // A `.rodata` base ANCHOR: when codegen addresses the read-only tables
    // through one section-relative base (s_atan's atanhi/atanlo/aT off a
    // single lis/addi pair), mwcc emits a zero-size LOCAL `STT_NOTYPE`
    // symbol named `...rodata.0` at `.rodata`+0 — created right after the
    // FIRST rodata object's symbol — and binds the ADDR16 relocations to
    // it. Only emitted when a relocation actually targets that name.
    let rodata_anchor_needed = input.functions.iter().any(|function| {
        function.relocations.iter().any(|relocation| {
            matches!(&relocation.target, RelocationTarget::External(name) if name == "...rodata.0")
        })
    });
    let mut rodata_anchor_emitted = false;
    if rodata_anchor_needed && input.object_format.rodata_anchor_before_data_symbols {
        local_data_symbols.insert("...rodata.0", (symtab.len() / SYMBOL_SIZE) as u32);
        write_symbol(
            &mut symtab,
            strtab.add("...rodata.0"),
            0,
            0,
            0,
            0,
            index_of(".rodata") as u16,
        );
        comment_values.push((1, input.object_format.rodata_anchor_comment_flags));
        rodata_anchor_emitted = true;
    }
    for object in &input.data_objects {
        if object.is_static
            && (static_forward(object)
                || input.object_format.local_data_symbols_in_declaration_order)
            && !function_string_names.contains(object.name)
            && object.static_local_owner.is_none()
            // Declared BETWEEN functions -> emitted at its source position in
            // the per-function run below (ansi_fp's `unused` + its string).
            // SECTION-ATTRIBUTED statics (gdc's `.dtors` reference) stay up
            // front regardless of declaration position (measured).
            && (object.functions_before == 0 || object.section.is_some())
        {
            if is_pending_zero_static(object) {
                emitted_zero_static.insert(object.name);
            }
            local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            write_symbol(
                &mut symtab,
                strtab.add(object.name),
                data_offsets[object.name],
                data_sizes[object.name],
                STB_LOCAL_OBJECT,
                0,
                section,
            );
            comment_values.push((data_aligns[object.name], 0));
            if rodata_anchor_needed
                && !rodata_anchor_emitted
                && data_section[object.name] == ".rodata"
            {
                local_data_symbols.insert("...rodata.0", (symtab.len() / SYMBOL_SIZE) as u32);
                write_symbol(&mut symtab, strtab.add("...rodata.0"), 0, 0, 0, 0, section);
                // .comment record (1, 0x00100000) — measured; the flag bit marks
                // the section-anchor entry.
                comment_values.push((1, input.object_format.rodata_anchor_comment_flags));
                rodata_anchor_emitted = true;
            }
        }
    }
    // Symbol bookkeeping is declared before the optional interleaving pin: a
    // pinned sequence can contain both zero statics and static functions.
    let mut function_symbols: Vec<u32> = vec![0u32; functions.len()];
    let mut local_function_symbols: std::collections::HashMap<&str, u32> =
        std::collections::HashMap::new();
    let mut emitted_early_func: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // An exact whole-TU capture may pin the legacy symbol-creation timeline:
    // zero statics materialize at first reference while static functions appear
    // at a prior declaration or definition, interleaved with those data symbols.
    // Names not in the pin continue through the general policies below.
    for name in input.local_symbol_order {
        if let Some(object) = input
            .data_objects
            .iter()
            .find(|object| object.name == name && is_pending_zero_static(object))
        {
            if emitted_zero_static.insert(object.name) {
                local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                let section = index_of(data_section[object.name]) as u16;
                write_symbol(
                    &mut symtab,
                    strtab.add(object.name),
                    data_offsets[object.name],
                    data_sizes[object.name],
                    STB_LOCAL_OBJECT,
                    0,
                    section,
                );
                comment_values.push((data_aligns[object.name], 0));
            }
            continue;
        }
        if let Some(index) = functions.iter().position(|function| {
            function.is_static && !function.implicit_local && function.name == name
        }) {
            if emitted_early_func.insert(index) {
                let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
                write_symbol(
                    &mut symtab,
                    strtab.add(functions[index].name),
                    function_offset[index],
                    function_size[index],
                    STB_LOCAL_FUNC,
                    0,
                    index_of(text_section) as u16,
                );
                comment_values.push((
                    4,
                    if functions[index].force_active {
                        FORCE_ACTIVE_FLAG
                    } else {
                        0
                    },
                ));
                function_symbols[index] = symbol;
                local_function_symbols.insert(functions[index].name, symbol);
            }
        }
    }
    // Then the remaining zero statics (uninitialized `.sbss`, or any `.bss`):
    // REFERENCED ones first, in first-reference order across the functions
    // (their symbol_order, falling back to relocation order — measured:
    // wind_waker abort_exit interleaves __atexit_funcs by its first use),
    // then any unreferenced ones in REVERSE declaration order.
    for function in functions {
        let relocation_names =
            function
                .relocations
                .iter()
                .filter_map(|relocation| match &relocation.target {
                    RelocationTarget::External(name)
                    | RelocationTarget::ExternalWithAddend(name, _) => Some(name.as_str()),
                    _ => None,
                });
        // TEXT-RELOCATION order, not symbol_order: mwcc's scheduler hoists a
        // loop-invariant table base (lis/addi) ABOVE the loop guard, and the
        // zero-static symbol run follows the FIRST TEXT REFERENCE (measured:
        // wind_waker abort_exit — __atexit_funcs before __atexit_curr_func,
        // opposite to the AST order symbol_order carries).
        for name in relocation_names {
            if let Some(object) = input
                .data_objects
                .iter()
                .find(|object| object.name == name && is_pending_zero_static(object))
            {
                if emitted_zero_static.insert(object.name) {
                    local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                    let section = index_of(data_section[object.name]) as u16;
                    write_symbol(
                        &mut symtab,
                        strtab.add(object.name),
                        data_offsets[object.name],
                        data_sizes[object.name],
                        STB_LOCAL_OBJECT,
                        0,
                        section,
                    );
                    comment_values.push((data_aligns[object.name], 0));
                }
            }
        }
    }
    for object in input.data_objects.iter().rev() {
        if is_pending_zero_static(object) && !emitted_zero_static.contains(object.name) {
            local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            write_symbol(
                &mut symtab,
                strtab.add(object.name),
                data_offsets[object.name],
                data_sizes[object.name],
                STB_LOCAL_OBJECT,
                0,
                section,
            );
            comment_values.push((data_aligns[object.name], 0));
        }
    }
    // `static` functions are file-local: a LOCAL `STT_FUNC` symbol each, in
    // declaration order, after the static data and before the functions' `@N`
    // entries (mwcc emits `static int f(){…}` here, ahead of any unwind `@N`). Their
    // symbol indices are recorded by function index so a call relocation resolves to
    // the local symbol; the global run below skips them.
    let mut static_slot_symbols: HashMap<(usize, usize), u32> = HashMap::new();
    let mut static_slot_symbol_by_value: HashMap<(u64, u8), u32> = HashMap::new();
    // One `.sdata2` symbol per distinct constant — declared here so the EARLY
    // image emission registers into the same dedup map an ordinary pool
    // reference consults (ww's wcstombs reuses unicode's @47 image plainly).
    let mut constant_symbol: HashMap<(u64, u8), u32> = HashMap::new();
    // The `...data.0` SECTION-ALIAS marker is emitted LAZILY: immediately
    // before the unit's first `.data`-section string/object symbol (measured:
    // strikers — after 5 static FUNC symbols, before @229; wind_waker — after
    // ctzl's symbol, before @797).
    let mut data_marker_pending = functions.iter().any(|function| {
        function.relocations.iter().any(|relocation| matches!(&relocation.target, RelocationTarget::External(name) if name == "...data.0"))
    });
    // ONE creation-order pass: per function, its file-scope statics, static
    // locals, strings, blobs, pooled constants, unwind entries, jump table,
    // then — for a STATIC function — its own FUNC symbol at definition end
    // (measured: mbstring's @4 image, ansi_fp's @837 pool and @752 table all
    // LEAD their static owners' symbols; ww's `dummy` symbol sits BETWEEN
    // two_exp's and num2dec_internal's @N blocks).
    let mut constant_symbols: Vec<Vec<u32>> = Vec::new();
    let mut extab_entry_symbols: Vec<u32> = Vec::new();
    let mut jump_table_symbols: Vec<Vec<u32>> = Vec::new();
    let mut rodata_blob_symbols: Vec<Vec<u32>> = Vec::new();
    // A `static` function forward-declared by a prototype had its LOCAL FUNC
    // symbol created at that first declaration — so it emits here (after the
    // static data run, before the per-function `@N`/FUNC blocks), in prototype
    // order, ahead of statics first seen at their definition. The per-function
    // loop below skips any function emitted here (measured: OSAlarm's
    // `DecrementerExceptionHandler`, prototyped at the top of the file).
    for name in input.forward_declared_statics {
        if let Some(index) = functions.iter().position(|function| {
            function.is_static && !function.implicit_local && function.name == name.as_str()
        }) {
            if emitted_early_func.insert(index) {
                let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
                write_symbol(
                    &mut symtab,
                    strtab.add(functions[index].name),
                    function_offset[index],
                    function_size[index],
                    STB_LOCAL_FUNC,
                    0,
                    index_of(text_section) as u16,
                );
                comment_values.push((
                    4,
                    if functions[index].force_active {
                        FORCE_ACTIVE_FLAG
                    } else {
                        0
                    },
                ));
                function_symbols[index] = symbol;
                local_function_symbols.insert(functions[index].name, symbol);
            }
        }
    }
    for (index, function) in functions.iter().enumerate() {
        // FILE-SCOPE statics declared right before this function (ansi_fp's
        // `static const char* const unused = "…"` between function bodies)
        // emit at their source position, in declaration order — the string
        // object then its pointer.
        for object in &input.data_objects {
            if object.is_static
                && static_forward(object)
                && !function_string_names.contains(object.name)
                && object.static_local_owner.is_none()
                && object.section.is_none()
                // At its declaring position; a tail declaration (after the
                // last function) clamps to the final block.
                && (object.functions_before == index
                    || (index + 1 == functions.len() && object.functions_before > index))
                && index > 0
                && !local_data_symbols.contains_key(object.name)
            {
                if data_marker_pending && data_section[object.name] == ".data" {
                    local_data_symbols.insert("...data.0", (symtab.len() / SYMBOL_SIZE) as u32);
                    write_symbol(
                        &mut symtab,
                        strtab.add("...data.0"),
                        0,
                        0,
                        0,
                        0,
                        index_of(".data") as u16,
                    );
                    comment_values.push((1, 0x0010_0000));
                    data_marker_pending = false;
                }
                local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                let section = index_of(data_section[object.name]) as u16;
                write_symbol(
                    &mut symtab,
                    strtab.add(object.name),
                    data_offsets[object.name],
                    data_sizes[object.name],
                    STB_LOCAL_OBJECT,
                    0,
                    section,
                );
                comment_values.push((data_aligns[object.name], 0));
                // The `...rodata.0` anchor also follows the FIRST .rodata
                // static in the INTERLEAVED source-position run (pikmin
                // e_pow's `bp`, declared after scalbn).
                if rodata_anchor_needed
                    && !rodata_anchor_emitted
                    && data_section[object.name] == ".rodata"
                {
                    local_data_symbols.insert("...rodata.0", (symtab.len() / SYMBOL_SIZE) as u32);
                    write_symbol(&mut symtab, strtab.add("...rodata.0"), 0, 0, 0, 0, section);
                    comment_values.push((1, input.object_format.rodata_anchor_comment_flags));
                    rodata_anchor_emitted = true;
                }
            }
        }
        // An IMPLICIT function's STATIC LOCALS lead its block (its FUNC symbol
        // trails them — measured: ww uart). A REGULAR static function's locals
        // instead FOLLOW its FUNC symbol (measured: ac uart's initialized$16
        // after __init_uart_console).
        for object in &input.data_objects {
            if !function.implicit_local && function.is_static {
                break;
            }
            if object.static_local_owner == Some(index) {
                local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                let section = index_of(data_section[object.name]) as u16;
                let display = match static_local_numbers.get(object.name) {
                    Some(&number) => strtab.add(&format!("{}${}", object.name, number)),
                    None => strtab.add(object.name),
                };
                write_symbol(
                    &mut symtab,
                    display,
                    data_offsets[object.name],
                    data_sizes[object.name],
                    STB_LOCAL_OBJECT,
                    0,
                    section,
                );
                comment_values.push((data_aligns[object.name], 0));
                // The `...rodata.0` anchor also follows the FIRST .rodata
                // static LOCAL (pikmin inverse_trig's atan_coeff$N).
                if rodata_anchor_needed
                    && !rodata_anchor_emitted
                    && data_section[object.name] == ".rodata"
                {
                    local_data_symbols.insert("...rodata.0", (symtab.len() / SYMBOL_SIZE) as u32);
                    write_symbol(&mut symtab, strtab.add("...rodata.0"), 0, 0, 0, 0, section);
                    comment_values.push((1, input.object_format.rodata_anchor_comment_flags));
                    rodata_anchor_emitted = true;
                }
            }
        }
        // The implicit-materialization's LOCAL FUNC symbol trails its own
        // static locals (mwcc created the fn symbol at the late definition,
        // after compiling the body that declared them). Recorded for the
        // `.mwcats` relocation, NOT for call resolution (calls bind the ghost).
        if function.implicit_local {
            function_symbols[index] = (symtab.len() / SYMBOL_SIZE) as u32;
            write_symbol(
                &mut symtab,
                strtab.add(function.name),
                function_offset[index],
                function_size[index],
                STB_LOCAL_FUNC,
                0,
                index_of(text_section) as u16,
            );
            comment_values.push((4, 0)); // a function is 4-aligned
        }
        // This function's NEW strings sit at the FRONT of its `@N` block, before its constants and
        // unwind entries. Each `@N` name already has a laid-out data object (`.sdata`/`.data`); emit
        // its LOCAL symbol here and record it so relocations (this function's, and a later function
        // reusing the same pooled string) resolve to it.
        // (When string_number_after_constants / string_number_after_rodata is
        // set, the string SYMBOLS also emit at the deferred position — handled
        // inside the constants loop below / the blob loop.)
        if function.string_number_after_constants.is_none()
            && function.string_number_after_rodata.is_none()
        {
            for name in &function.string_names {
                if data_marker_pending && data_section[name.as_str()] == ".data" {
                    local_data_symbols.insert("...data.0", (symtab.len() / SYMBOL_SIZE) as u32);
                    write_symbol(
                        &mut symtab,
                        strtab.add("...data.0"),
                        0,
                        0,
                        0,
                        0,
                        index_of(".data") as u16,
                    );
                    comment_values.push((1, 0x0010_0000));
                    data_marker_pending = false;
                }
                local_data_symbols.insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                let section = index_of(data_section[name.as_str()]) as u16;
                write_symbol(
                    &mut symtab,
                    strtab.add(name),
                    data_offsets[name.as_str()],
                    data_sizes[name.as_str()],
                    STB_LOCAL_OBJECT,
                    0,
                    section,
                );
                comment_values.push((data_aligns[name.as_str()], 0));
            }
        }
        // The anonymous rodata blob's LOCAL `@N` symbol precedes the pool
        // constants' (symtab order measured on __strtold: @26 then @147).
        {
            let mut symbols_of_blobs = Vec::new();
            for (blob_index, &number) in rodata_blob_numbers[index].iter().enumerate() {
                // string_number_after_rodata: the string symbols emit between
                // blob K-1 and blob K, matching their numbering.
                if let Some((position, _)) = function.string_number_after_rodata {
                    if position == blob_index as u32 {
                        for name in &function.string_names {
                            local_data_symbols
                                .insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                            let section = index_of(data_section[name.as_str()]) as u16;
                            write_symbol(
                                &mut symtab,
                                strtab.add(name),
                                data_offsets[name.as_str()],
                                data_sizes[name.as_str()],
                                STB_LOCAL_OBJECT,
                                0,
                                section,
                            );
                            comment_values.push((data_aligns[name.as_str()], 0));
                        }
                    }
                }
                symbols_of_blobs.push((symtab.len() / SYMBOL_SIZE) as u32);
                let name = strtab.add(&format!("@{}", number));
                let size = function.anonymous_rodata[blob_index].0.len() as u32;
                write_symbol(
                    &mut symtab,
                    name,
                    rodata_blob_offset[index][blob_index],
                    size,
                    STB_LOCAL_OBJECT,
                    0,
                    index_of(".rodata") as u16,
                );
                // The blob's `.comment` alignment record is 4 (measured on __strtold's @26).
                comment_values.push((4, 0));
            }
            if let Some((position, _)) = function.string_number_after_rodata {
                if position as usize >= function.anonymous_rodata.len() {
                    for name in &function.string_names {
                        local_data_symbols
                            .insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                        let section = index_of(data_section[name.as_str()]) as u16;
                        write_symbol(
                            &mut symtab,
                            strtab.add(name),
                            data_offsets[name.as_str()],
                            data_sizes[name.as_str()],
                            STB_LOCAL_OBJECT,
                            0,
                            section,
                        );
                        comment_values.push((data_aligns[name.as_str()], 0));
                    }
                }
            }
            rodata_blob_symbols.push(symbols_of_blobs);
        }
        let mut symbols = Vec::new();
        for (constant_index, constant) in function.constants.iter().enumerate() {
            if function.string_number_after_constants == Some(constant_index as u32) {
                for name in &function.string_names {
                    local_data_symbols.insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                    let section = index_of(data_section[name.as_str()]) as u16;
                    write_symbol(
                        &mut symtab,
                        strtab.add(name),
                        data_offsets[name.as_str()],
                        data_sizes[name.as_str()],
                        STB_LOCAL_OBJECT,
                        0,
                        section,
                    );
                    comment_values.push((data_aligns[name.as_str()], 0));
                }
            }
            // An IMAGE constant's symbol already emitted ahead of its owning
            // static function — a later function reusing it binds the same
            // symbol.
            if constant.image {
                if let Some(&early) =
                    static_slot_symbol_by_value.get(&(constant.bits, constant.byte_width))
                {
                    symbols.push(early);
                    continue;
                }
            }
            match constant_symbol.get(&(constant.bits, constant.byte_width)) {
                Some(&existing) if !constant.force_new => symbols.push(existing),
                _ => {
                    let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
                    if !constant.force_new {
                        constant_symbol.insert((constant.bits, constant.byte_width), symbol);
                    }
                    symbols.push(symbol);
                    let name = strtab.add(&format!("@{}", constant_numbers[index][constant_index]));
                    write_symbol(
                        &mut symtab,
                        name,
                        constant_offsets[index][constant_index],
                        constant.byte_width as u32,
                        STB_LOCAL_OBJECT,
                        0,
                        index_of(".sdata2") as u16,
                    );
                    comment_values.push((
                        if constant.byte_width == 8 && constant.static_slot {
                            4
                        } else {
                            constant.byte_width as u32
                        },
                        0,
                    ));
                }
            }
        }
        constant_symbols.push(symbols);
        // A split point at (or past) the END of the constant list emits the
        // string symbols after the last constant (mirrors the numbering walk's
        // post-loop catch above).
        if let Some(position) = function.string_number_after_constants {
            if position as usize >= function.constants.len() {
                for name in &function.string_names {
                    local_data_symbols.insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                    let section = index_of(data_section[name.as_str()]) as u16;
                    write_symbol(
                        &mut symtab,
                        strtab.add(name),
                        data_offsets[name.as_str()],
                        data_sizes[name.as_str()],
                        STB_LOCAL_OBJECT,
                        0,
                        section,
                    );
                    comment_values.push((data_aligns[name.as_str()], 0));
                }
            }
        }
        if let Some(frame) = &frame_numbers[index] {
            extab_entry_symbols.push((symtab.len() / SYMBOL_SIZE) as u32);
            let extab_name = strtab.add(&format!("@{}", frame.extab));
            write_symbol(
                &mut symtab,
                extab_name,
                frame.extab_entry_offset,
                8,
                STB_LOCAL_OBJECT,
                STV_HIDDEN,
                index_of("extab") as u16,
            );
            let extabindex_name = strtab.add(&format!("@{}", frame.extabindex));
            write_symbol(
                &mut symtab,
                extabindex_name,
                frame.extabindex_entry_offset,
                12,
                STB_LOCAL_OBJECT,
                STV_HIDDEN,
                index_of("extabindex") as u16,
            );
            // The unwind entries are 4-aligned objects.
            comment_values.push((4, 0));
            comment_values.push((4, 0));
        } else {
            extab_entry_symbols.push(0);
        }
        // Each jump table is a 4-aligned local `@N` object in `.data`,
        // symbols in creation (numbering) order.
        let mut symbols_of_tables = Vec::new();
        for (table_index, table) in function.jump_tables.iter().enumerate() {
            symbols_of_tables.push((symtab.len() / SYMBOL_SIZE) as u32);
            let name = strtab.add(&format!("@{}", jump_table_numbers[index][table_index]));
            let size = table.entries.len() as u32 * 4;
            write_symbol(
                &mut symtab,
                name,
                jump_table_offset[index][table_index].unwrap(),
                size,
                STB_LOCAL_OBJECT,
                0,
                index_of(".data") as u16,
            );
            comment_values.push((4, 0));
        }
        jump_table_symbols.push(symbols_of_tables);
        // A STATIC function's own FUNC symbol closes its block (definition end;
        // an implicit-local already emitted after its static locals above),
        // followed by its own static locals (measured: ac uart) — unless
        // static_locals_lead flips the pair (mp4 alloc's get_malloc_pool:
        // protopool$129, init$130, then the FUNC).
        if function.is_static && !function.implicit_local && !emitted_early_func.contains(&index) {
            let emit_func = |symtab: &mut Vec<u8>,
                             strtab: &mut StringTable,
                             comment_values: &mut Vec<(u32, u32)>| {
                let symbol = (symtab.len() / SYMBOL_SIZE) as u32;
                write_symbol(
                    symtab,
                    strtab.add(function.name),
                    function_offset[index],
                    function_size[index],
                    STB_LOCAL_FUNC,
                    0,
                    index_of(text_section) as u16,
                );
                comment_values.push((
                    4,
                    if function.force_active {
                        FORCE_ACTIVE_FLAG
                    } else {
                        0
                    },
                )); // a function is 4-aligned
                symbol
            };
            if !function.static_locals_lead {
                let symbol = emit_func(&mut symtab, &mut strtab, &mut comment_values);
                function_symbols[index] = symbol;
                local_function_symbols.insert(function.name, symbol);
            }
            // Owned statics emit in REVERSE declaration order (measured:
            // init$130 then protopool$129 on alloc.c; init$110 then
            // protopool$109 on _alloc.c's default FUNC-first path).
            let owned: Vec<&DataObject> = input.data_objects.iter().rev().collect();
            for object in owned {
                if object.static_local_owner == Some(index) {
                    local_data_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                    let section = index_of(data_section[object.name]) as u16;
                    let display = match static_local_numbers.get(object.name) {
                        Some(&number) => strtab.add(&format!("{}${}", object.name, number)),
                        None => strtab.add(object.name),
                    };
                    write_symbol(
                        &mut symtab,
                        display,
                        data_offsets[object.name],
                        data_sizes[object.name],
                        STB_LOCAL_OBJECT,
                        0,
                        section,
                    );
                    comment_values.push((data_aligns[object.name], 0));
                    // The `...rodata.0` anchor also follows the FIRST .rodata
                    // static LOCAL (pikmin inverse_trig's atan_coeff$N).
                    if rodata_anchor_needed
                        && !rodata_anchor_emitted
                        && data_section[object.name] == ".rodata"
                    {
                        local_data_symbols
                            .insert("...rodata.0", (symtab.len() / SYMBOL_SIZE) as u32);
                        write_symbol(&mut symtab, strtab.add("...rodata.0"), 0, 0, 0, 0, section);
                        comment_values.push((1, input.object_format.rodata_anchor_comment_flags));
                        rodata_anchor_emitted = true;
                    }
                }
            }
            if function.static_locals_lead {
                let symbol = emit_func(&mut symtab, &mut strtab, &mut comment_values);
                function_symbols[index] = symbol;
                local_function_symbols.insert(function.name, symbol);
            }
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
    // One initialized exported object's symbol plus its pointer-relocation
    // targets (reverse element order) — shared by the up-front run and the
    // source-position interleaved runs below.
    // The reverse-slot, first-seen walk of ONE object's relocation targets — shared
    // by the full initialized-object emission and the STATIC-table hook (whose own
    // symbol lives in the LOCAL run; only its hoisted targets join the GLOBAL run).
    macro_rules! emit_object_targets {
        ($object:expr) => {{
            let object = $object;
            for relocation in object.relocations.iter().rev() {
                let target = relocation.target.as_str();
                // A STATIC function's LOCAL symbol satisfies a data reloc too —
                // pikmin trigf's `.ctors` reference to its static sinit must
                // not mint a spurious UND duplicate.
                if global_symbols.contains_key(target)
                    || local_data_symbols.contains_key(target)
                    || local_function_symbols.contains_key(target)
                {
                    continue;
                }
                global_symbols.insert(target, (symtab.len() / SYMBOL_SIZE) as u32);
                if let Some(&offset) = data_offsets.get(target) {
                    write_symbol(
                        &mut symtab,
                        strtab.add(target),
                        offset,
                        data_sizes[target],
                        STB_GLOBAL_OBJECT,
                        0,
                        index_of(data_section[target]) as u16,
                    );
                    comment_values.push((data_aligns[target], 0));
                } else if let Some(function_index) = functions
                    .iter()
                    .position(|function| !function.is_static && function.name == target)
                {
                    // A unit FUNCTION address-taken by this object (a dispatch table
                    // referencing functions defined later): mwcc hoists its GLOBAL
                    // FUNC symbol to the data object's position — this reverse-slot,
                    // first-seen loop reproduces the measured order (tbl, e3, e2, e1).
                    // The per-function run below skips it via the global_symbols guard.
                    let function = &functions[function_index];
                    let binding = if function.is_weak {
                        STB_WEAK_FUNC
                    } else {
                        STB_GLOBAL_FUNC
                    };
                    function_symbols[function_index] = (symtab.len() / SYMBOL_SIZE) as u32;
                    write_symbol(
                        &mut symtab,
                        strtab.add(function.name),
                        function_offset[function_index],
                        function_size[function_index],
                        binding,
                        0,
                        index_of(text_section) as u16,
                    );
                    let flags = if function.is_weak {
                        if function.weak_inline {
                            0x0d00_0000
                        } else {
                            0x0e00_0000
                        }
                    } else {
                        0
                    };
                    comment_values.push((
                        4,
                        flags
                            | if function.force_active {
                                FORCE_ACTIVE_FLAG
                            } else {
                                0
                            },
                    ));
                } else {
                    write_symbol(
                        &mut symtab,
                        strtab.add(target),
                        0,
                        0,
                        STB_GLOBAL_NOTYPE,
                        0,
                        SHN_UNDEF,
                    );
                    comment_values.push((0, 0));
                }
            }
        }};
    }
    macro_rules! emit_initialized_object {
        ($object:expr) => {{
            let object = $object;
            // An ANONYMOUS object (the synthesized `.ctors` sinit reference)
            // lays out and relocates but has NO symbol or .comment record.
            if !object.name.is_empty() {
                global_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
                let section = index_of(data_section[object.name]) as u16;
                let binding = if object.is_weak { STB_WEAK_OBJECT } else { STB_GLOBAL_OBJECT };
                // A weak OBJECT's .comment flags are 0x0d (a weak FUNCTION carries 0x0e — measured).
                let flags = if object.is_weak { 0x0d00_0000 } else { 0 };
                write_symbol(&mut symtab, strtab.add(object.name), data_offsets[object.name], data_sizes[object.name], binding, 0, section);
                comment_values.push((data_aligns[object.name], flags));
            }
            emit_object_targets!(object);
        }};
    }
    // A STATIC dispatch table (`static void (*tbl[])(void) = { e1, e2 };`): its own
    // symbol binds LOCAL (emitted in the local-statics run above), but its hoisted
    // relocation targets join the GLOBAL run at the table's source position — up
    // front here, or after function K below (measured: mid-file table gives
    // f, e2, e1, h).
    let is_static_table_hook = |object: &DataObject| -> bool {
        object.is_static
            && !object.is_const
            && object.section.is_none()
            && object.static_local_owner.is_none()
            && !object.relocations.is_empty()
    };
    let is_initialized_run_object = |object: &DataObject| -> bool {
        let section_name = data_section[object.name];
        // `.ctors`/`.dtors` chain references join the run: their symbols emit in
        // FORWARD declaration order with the undefined-symbol fallback for their
        // relocation targets (measured: the UNDEF `__destroy_global_chain` lands
        // right after the `.dtors` object referencing it).
        !object.is_static
            && (matches!(
                section_name,
                ".sdata" | ".data" | ".sdata2" | ".rodata" | ".ctors" | ".dtors"
            ) || (section_name == ".sbss" && object.is_explicit_zero))
    };
    // The always-present initialized sections (`.sdata`/`.data`, and the read-only
    // `.sdata2`/`.rodata`) emit their symbols up front in declaration order; the
    // zero `.sbss`/`.bss` objects instead follow reference order (handled below).
    // The initialized sections AND the EXPLICITLY zero-initialized small `.sbss`
    // globals (`int a = 0;`) emit their symbols in DECLARATION order at their
    // SOURCE POSITION: objects declared before any non-static function up front
    // here, later ones interleaved after that function's symbol below (mwcc:
    // `__lower_map, tolower, __upper_map` — the ctype shape). Only the
    // UNINITIALIZED zero globals trail the functions in reverse. (Large `.bss`
    // follows its own reference-order rule, untouched here.)
    for object in &input.data_objects {
        if is_initialized_run_object(object) && object.functions_before == 0 {
            emit_initialized_object!(object);
        } else if is_static_table_hook(object) && object.functions_before == 0 {
            emit_object_targets!(object);
        }
    }
    let mut functions_seen = 0usize;
    for (index, function) in functions.iter().enumerate() {
        // Assign this function's referenced externals in mwcc's symbol-table order
        // (its AST `symbol_order`) for the names it lists, then any remaining in
        // relocation order so nothing is missed. `.text` reference (offset) order
        // does not match mwcc's symbol order, so we cannot key off the relocations.
        let external_targets: std::collections::HashSet<&str> = function
            .relocations
            .iter()
            .filter_map(|relocation| match &relocation.target {
                RelocationTarget::External(name)
                | RelocationTarget::ExternalWithAddend(name, _) => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let mut ordered: Vec<&str> = Vec::new();
        let mut listed = std::collections::HashSet::new();
        // Phantom externals first (created before the function's own refs).
        for name in &function.phantom_externals {
            if listed.insert(name.as_str()) {
                ordered.push(name.as_str());
            }
        }
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
        // An IMPLICITLY-declared callee's symbol is created by mwcc at its call site inside
        // the body, so it is emitted AFTER the function symbol; an explicitly-declared
        // (prototyped) external precedes it. Partition preserving order within each group.
        // A CALLED inline-asm helper (static inline, skipped) was DECLARED
        // before the function — mwcc orders it with the prototyped externals,
        // not the implicit call-site run (measured: pikmin s_ldexp).
        let local_callees: std::collections::HashSet<&str> = function
            .local_undefined_callees
            .iter()
            .map(|name| name.as_str())
            .collect();
        let implicit: std::collections::HashSet<&str> = function
            .implicit_external_callees
            .iter()
            .map(|name| name.as_str())
            .filter(|name| !referenced_inline_asm.contains(name) && !local_callees.contains(name))
            .collect();
        let (implicit_ordered, explicit_ordered): (Vec<&str>, Vec<&str>) = ordered
            .into_iter()
            .partition(|name| implicit.contains(name));
        let early_implicit: std::collections::HashSet<&str> = function
            .early_implicit_external_callees
            .iter()
            .map(|name| name.as_str())
            .collect();
        let (early_implicit_ordered, implicit_ordered): (Vec<&str>, Vec<&str>) =
            implicit_ordered
                .into_iter()
                .partition(|name| early_implicit.contains(name));
        // Build 163 creates an absolute-address symbol while materializing its
        // ADDR16 pair, before it registers the current function. SDA21 data
        // references and calls retain the ordinary function-first ordering.
        let absolute_targets: std::collections::HashSet<&str> = function
            .relocations
            .iter()
            .filter(|relocation| matches!(relocation.elf_type, 4 | 5 | 6))
            .filter_map(|relocation| match &relocation.target {
                RelocationTarget::External(name)
                | RelocationTarget::ExternalWithAddend(name, _)
                    if !function
                        .referenced_function_symbols
                        .iter()
                        .any(|function_name| function_name == name) =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect();
        let (absolute_ordered, explicit_ordered): (Vec<&str>, Vec<&str>) =
            if input.object_format.function_symbol_before_references {
                explicit_ordered
                    .into_iter()
                    .partition(|name| absolute_targets.contains(name))
            } else {
                (Vec::new(), explicit_ordered)
            };
        // The register save/restore HELPERS (_savegpr_N/_restgpr_N) are created
        // while mwcc compiles the PROLOGUE/EPILOGUE — before the function's
        // symbol — even though they are unprototyped (measured: strtoul).
        let (helper_ordered, implicit_ordered): (Vec<&str>, Vec<&str>) =
            implicit_ordered.into_iter().partition(|name| {
                name.starts_with("_savegpr_")
                    || name.starts_with("_restgpr_")
                    || name.starts_with("_savefpr_")
                    || name.starts_with("_restfpr_")
            });
        // Emit one external/global symbol (skipping a name that already resolves to an
        // existing global or LOCAL `static` symbol). A `macro_rules!` keeps the shared body
        // in one place while avoiding a closure over the many `&mut` writer collections.
        macro_rules! emit_referenced {
            ($names:expr) => {
                for name in $names {
                    if global_symbols.contains_key(name)
                        || local_data_symbols.contains_key(name)
                        || local_function_symbols.contains_key(name)
                    {
                        continue;
                    }
                    // A call to a function defined LATER in this unit emits
                    // the DEFINED symbol at the first-reference position — in
                    // the caller's EXPLICIT run when prototyped (mp4 file_io:
                    // fflush FUNC before fclose's own symbol) or its IMPLICIT
                    // run when not (AC: [fclose, fflush FUNC, free UND]) — the
                    // definition's own position run then skips it.
                    if let Some(forward) = functions.iter().position(|later| {
                        !later.is_static && !later.implicit_local && later.name == name
                    }) {
                        global_symbols.insert(name, (symtab.len() / SYMBOL_SIZE) as u32);
                        function_symbols[forward] = (symtab.len() / SYMBOL_SIZE) as u32;
                        let binding = if functions[forward].is_weak {
                            STB_WEAK_FUNC
                        } else {
                            STB_GLOBAL_FUNC
                        };
                        write_symbol(
                            &mut symtab,
                            strtab.add(name),
                            function_offset[forward],
                            function_size[forward],
                            binding,
                            0,
                            index_of(text_section) as u16,
                        );
                        let flags = if functions[forward].is_weak {
                            if functions[forward].weak_inline {
                                0x0d00_0000
                            } else {
                                0x0e00_0000
                            }
                        } else {
                            0
                        };
                        comment_values.push((4, flags));
                        continue;
                    }
                    global_symbols.insert(name, (symtab.len() / SYMBOL_SIZE) as u32);
                    if let Some(&offset) = data_offsets.get(name) {
                        let section = index_of(data_section[name]) as u16;
                        write_symbol(
                            &mut symtab,
                            strtab.add(name),
                            offset,
                            data_sizes[name],
                            STB_GLOBAL_OBJECT,
                            0,
                            section,
                        );
                        comment_values.push((data_aligns[name], 0));
                    } else if referenced_inline_asm.contains(name) {
                        // a CALLED static-inline asm helper stays LOCAL (info 0).
                        write_symbol(&mut symtab, strtab.add(name), 0, 0, 0, 0, SHN_UNDEF);
                        comment_values.push((0, 0));
                    } else {
                        write_symbol(
                            &mut symtab,
                            strtab.add(name),
                            0,
                            0,
                            STB_GLOBAL_NOTYPE,
                            0,
                            SHN_UNDEF,
                        );
                        comment_values.push((0, 0)); // an undefined external has no alignment
                    }
                }
            };
        }
        macro_rules! emit_current_function_symbol {
            () => {
                if !function.is_static && !global_symbols.contains_key(function.name) {
                    function_symbols[index] = (symtab.len() / SYMBOL_SIZE) as u32;
                    let binding = if function.is_weak {
                        STB_WEAK_FUNC
                    } else {
                        STB_GLOBAL_FUNC
                    };
                    write_symbol(
                        &mut symtab,
                        strtab.add(function.name),
                        function_offset[index],
                        function_size[index],
                        binding,
                        0,
                        index_of(text_section) as u16,
                    );
                    let flags = if function.is_weak {
                        if function.weak_inline {
                            0x0d00_0000
                        } else {
                            0x0e00_0000
                        }
                    } else {
                        0
                    };
                    comment_values.push((
                        4,
                        flags
                            | if function.force_active {
                                FORCE_ACTIVE_FLAG
                            } else {
                                0
                            },
                    ));
                    global_symbols.insert(function.name, function_symbols[index]);
                    for (name, byte_offset) in &function.entry_points {
                        global_symbols.insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                        write_symbol(
                            &mut symtab,
                            strtab.add(name),
                            function_offset[index] + byte_offset,
                            0,
                            STB_GLOBAL_NOTYPE,
                            0,
                            index_of(text_section) as u16,
                        );
                        comment_values.push((
                            4,
                            if function.force_active {
                                FORCE_ACTIVE_FLAG
                            } else {
                                0
                            },
                        ));
                    }
                }
            };
        }
        if input.object_format.function_symbol_before_references {
            emit_referenced!(absolute_ordered);
            emit_current_function_symbol!();
            emit_referenced!(early_implicit_ordered.iter().copied());
        }
        // Prototyped externals first, then the save/restore helpers, then the
        // function's own symbol, then the remaining implicit callees.
        emit_referenced!(explicit_ordered);
        emit_referenced!(helper_ordered);
        // A `static` function already has its LOCAL symbol (emitted above); only its
        // newly-referenced externals appear in this run, not the function symbol.
        emit_current_function_symbol!();
        if !input.object_format.function_symbol_before_references {
            emit_referenced!(early_implicit_ordered.iter().copied());
        }
        // A STATIC asm function's own symbol is LOCAL (emitted in the local run
        // above); its GLOBAL `entry` points still emit HERE, at the function's source
        // position in the global run (wind_waker's `ASM static` runtime.c — the local
        // save/restore functions group early, their global entries follow the global
        // functions in source order).
        if function.is_static {
            for (name, byte_offset) in &function.entry_points {
                global_symbols.insert(name.as_str(), (symtab.len() / SYMBOL_SIZE) as u32);
                write_symbol(
                    &mut symtab,
                    strtab.add(name),
                    function_offset[index] + byte_offset,
                    0,
                    STB_GLOBAL_NOTYPE,
                    0,
                    index_of(text_section) as u16,
                );
                comment_values.push((
                    4,
                    if function.force_active {
                        FORCE_ACTIVE_FLAG
                    } else {
                        0
                    },
                ));
            }
        }
        emit_referenced!(implicit_ordered);
        // Initialized objects declared right after this function emit here —
        // the source-position interleaving. STATIC functions count too
        // (measured: ansi_fp's .rodata digit table, declared between static
        // functions, lands after the preceding function's referenced
        // externals, not up front).
        functions_seen += 1;
        for object in &input.data_objects {
            if is_initialized_run_object(object)
                && object.functions_before == functions_seen
                && !global_symbols.contains_key(object.name)
            {
                emit_initialized_object!(object);
            } else if is_static_table_hook(object) && object.functions_before == functions_seen {
                emit_object_targets!(object);
            }
        }
    }
    // Still-unreferenced (.sbss/.bss) defined globals trail the functions, in
    // REVERSE declaration order (verified: `int a;b;c;d;e;` -> `e d c b a`, and a
    // mixed .bss/.sbss set reverses too, independent of section). `static` objects
    // are local and never appear here.
    for object in input.data_objects.iter().rev() {
        if !object.is_static && !object.name.is_empty() && !global_symbols.contains_key(object.name)
        {
            global_symbols.insert(object.name, (symtab.len() / SYMBOL_SIZE) as u32);
            let section = index_of(data_section[object.name]) as u16;
            let binding = if object.is_weak {
                STB_WEAK_OBJECT
            } else {
                STB_GLOBAL_OBJECT
            };
            // A weak OBJECT's .comment flags are 0x0d (a weak FUNCTION carries 0x0e — measured).
            let flags = if object.is_weak { 0x0d00_0000 } else { 0 };
            write_symbol(
                &mut symtab,
                strtab.add(object.name),
                data_offsets[object.name],
                data_sizes[object.name],
                binding,
                0,
                section,
            );
            comment_values.push((data_aligns[object.name], flags));
        }
    }
    // The `.comment` trailer is now fully determined by the symbol alignments.
    let comment = comment_record(input.object_format.comment, &comment_values);

    // 3. Relocation payloads (now that symbol indices are fixed). Each function's
    //    `.text` relocations are rebased by its `.text` offset; a relocation
    //    targets either an external or one of that function's pooled constants.
    let mut rela_text = Vec::new();
    for &index in &layout_order {
        let function = &functions[index];
        for relocation in &function.relocations {
            let mut rela_addend: u32 = 0;
            let symbol = match &relocation.target {
                // A `static` target is a local data or function symbol; everything
                // else is a global/external symbol.
                RelocationTarget::External(name) => *local_data_symbols
                    .get(name.as_str())
                    .or_else(|| local_function_symbols.get(name.as_str()))
                    .unwrap_or_else(|| &global_symbols[name.as_str()]),
                RelocationTarget::ExternalWithAddend(name, addend) => {
                    rela_addend = *addend as u32;
                    *local_data_symbols
                        .get(name.as_str())
                        .or_else(|| local_function_symbols.get(name.as_str()))
                        .unwrap_or_else(|| &global_symbols[name.as_str()])
                }
                RelocationTarget::Constant(constant_index) => {
                    constant_symbols[index][*constant_index]
                }
                RelocationTarget::ConstantWithAddend(constant_index, addend) => {
                    rela_addend = *addend as u32;
                    constant_symbols[index][*constant_index]
                }
                RelocationTarget::JumpTable => jump_table_symbols[index][0],
                RelocationTarget::JumpTableAt(table_index) => {
                    jump_table_symbols[index][*table_index]
                }
                RelocationTarget::AnonymousRodata => rodata_blob_symbols[index][0],
                RelocationTarget::AnonymousRodataAt(blob_index) => {
                    rodata_blob_symbols[index][*blob_index]
                }
            };
            write_rela(
                &mut rela_text,
                function_offset[index] + relocation.offset,
                symbol,
                relocation.elf_type,
                rela_addend,
            );
        }
    }
    // `.rela.data` — each jump-table entry is an `ADDR32` to its function with the
    // case body's byte offset as the addend.
    let mut rela_data = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        // Tables emit their entry relocations in LAYOUT order (ascending
        // section offset), each an ADDR32 to the function + body offset.
        let mut ordered: Vec<usize> = (0..function.jump_tables.len()).collect();
        ordered.sort_by_key(|&table_index| jump_table_offset[index][table_index]);
        for table_index in ordered {
            let table = &function.jump_tables[table_index];
            let base = jump_table_offset[index][table_index].unwrap();
            for (entry_index, &body_offset) in table.entries.iter().enumerate() {
                write_rela(
                    &mut rela_data,
                    base + entry_index as u32 * 4,
                    function_symbols[index],
                    R_PPC_ADDR32,
                    body_offset,
                );
            }
        }
    }
    let mut rela_extabindex = Vec::new();
    for (index, frame) in frame_numbers.iter().enumerate() {
        if let Some(frame) = frame {
            write_rela(
                &mut rela_extabindex,
                frame.extabindex_entry_offset,
                function_symbols[index],
                R_PPC_ADDR32,
                0,
            ); // -> the function
            write_rela(
                &mut rela_extabindex,
                frame.extabindex_entry_offset + 8,
                extab_entry_symbols[index],
                R_PPC_ADDR32,
                0,
            ); // -> its extab entry
        }
    }
    // One `.mwcats` record + relocation per COMPILER-GENERATED function, packed
    // densely (asm functions are skipped, so the catalog position is not the
    // function index).
    let mut rela_mwcats = Vec::new();
    let mut mwcats_position = 0u32;
    for &index in &layout_order {
        let function = &functions[index];
        if function.is_asm {
            continue;
        }
        write_rela(
            &mut rela_mwcats,
            mwcats_position * 8 + 4,
            function_symbols[index],
            R_PPC_ADDR32,
            0,
        );
        mwcats_position += 1;
    }
    // `.rela.sdata`/`.rela.data` — a pointer global's `ADDR32` to the symbol it
    // points at, at the object's offset plus the relocation's own offset (plus
    // addend). Within one object (a pointer array) both the target symbols AND the
    // relocation entries run in REVERSE element order. `.sdata` for small pointers,
    // `.data` for large arrays.
    let mut rela_sdata = Vec::new();
    let resolve_data_target = |name: &str| -> u32 {
        *local_data_symbols
            .get(name)
            .or_else(|| local_function_symbols.get(name))
            .unwrap_or_else(|| &global_symbols[name])
    };
    for object in &input.data_objects {
        if data_section[object.name] != ".sdata" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(
                &mut rela_sdata,
                data_offsets[object.name] + relocation.offset,
                resolve_data_target(&relocation.target),
                R_PPC_ADDR32,
                relocation.addend as u32,
            );
        }
    }
    let mut rela_sdata2 = Vec::new();
    for object in &input.data_objects {
        if data_section[object.name] != ".sdata2" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(
                &mut rela_sdata2,
                data_offsets[object.name] + relocation.offset,
                resolve_data_target(&relocation.target),
                R_PPC_ADDR32,
                relocation.addend as u32,
            );
        }
    }
    for object in &input.data_objects {
        if data_section[object.name] != ".data" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(
                &mut rela_data,
                data_offsets[object.name] + relocation.offset,
                resolve_data_target(&relocation.target),
                R_PPC_ADDR32,
                relocation.addend as u32,
            );
        }
    }
    // `.rela.ctors`/`.rela.dtors`: each chain reference's `ADDR32` to its function.
    let mut rela_ctors = Vec::new();
    for object in &input.data_objects {
        if data_section[object.name] != ".ctors" {
            continue;
        }
        for relocation in object.relocations.iter() {
            write_rela(
                &mut rela_ctors,
                data_offsets[object.name] + relocation.offset,
                resolve_data_target(&relocation.target),
                R_PPC_ADDR32,
                relocation.addend as u32,
            );
        }
    }
    let mut rela_dtors = Vec::new();
    for object in &input.data_objects {
        if data_section[object.name] != ".dtors" {
            continue;
        }
        for relocation in object.relocations.iter().rev() {
            write_rela(
                &mut rela_dtors,
                data_offsets[object.name] + relocation.offset,
                resolve_data_target(&relocation.target),
                R_PPC_ADDR32,
                relocation.addend as u32,
            );
        }
    }

    // 4. Content payloads. One `.mwcats` record per function: `(0x02000000 | its
    //    text size, &function)`. The unwind header is a deterministic function of
    //    the saved-register shape; each `extabindex` entry is (function, function
    //    size, extab entry).
    let mut mwcats = Vec::new();
    for &index in &layout_order {
        let function = &functions[index];
        if function.is_asm {
            continue;
        }
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
    let offset_of =
        |name: &str| name_offsets[order.iter().position(|entry| *entry == name).unwrap()];

    // 6. Assemble the full section table (NULL first), each with its payload.
    let symtab_section = index_of(".symtab");
    let mut sections = vec![Section {
        name_offset: 0,
        sh_type: 0,
        flags: 0,
        link: 0,
        info: 0,
        align: 0,
        entry_size: 0,
        payload: Vec::new(),
        size: 0,
    }];
    // `mem_size` is the in-memory size; it overrides `payload.len()` only when the
    // payload is empty, which is how a NOBITS section (`.sbss`) carries a size with
    // no file bytes. Every other section passes 0 and takes its payload length.
    let mut push = |name: &str,
                    sh_type,
                    flags,
                    link,
                    info,
                    align,
                    entry_size,
                    payload: Vec<u8>,
                    mem_size: u32| {
        let size = if payload.is_empty() {
            mem_size
        } else {
            payload.len() as u32
        };
        sections.push(Section {
            name_offset: offset_of(name),
            sh_type,
            flags,
            link,
            info,
            align,
            entry_size,
            payload,
            size,
        });
    };
    if has_functions {
        push(
            text_section,
            SHT_PROGBITS,
            SHF_WRITE_EXEC,
            0,
            0,
            4,
            0,
            text.to_vec(),
            0,
        );
    }
    if has_frame {
        push("extab", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, extab, 0);
        push(
            "extabindex",
            SHT_PROGBITS,
            SHF_ALLOC,
            0,
            0,
            4,
            0,
            extabindex,
            0,
        );
    }
    // `.ctors`/`.dtors`: PROGBITS, ALLOC (read-only), 4-aligned — the chain
    // reference words (the `ADDR32` relocations live in `.rela.ctors`/`.rela.dtors`).
    if has_ctors {
        push(".ctors", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, ctors, 0);
    }
    if has_dtors {
        push(".dtors", SHT_PROGBITS, SHF_ALLOC, 0, 0, 4, 0, dtors, 0);
    }
    // `.rodata` (read-only const data) then the large writable `.data`/`.bss`
    // precede the small-data sections, matching the section-name order above.
    if has_rodata {
        push(
            ".rodata",
            SHT_PROGBITS,
            SHF_ALLOC,
            0,
            0,
            section_align(".rodata"),
            0,
            rodata,
            0,
        );
    }
    if has_data {
        // `.data` holds the creation-order layout computed above — file data
        // and jump tables interleaved; table bytes stay zero (ADDR32
        // relocations fill them at link time).
        push(
            ".data",
            SHT_PROGBITS,
            SHF_WRITE_ALLOC,
            0,
            0,
            section_align(".data"),
            0,
            file_data,
            0,
        );
    }
    if has_bss {
        // `.bss` is NOBITS (large zero-initialized globals): a size, no file bytes.
        push(
            ".bss",
            SHT_NOBITS,
            SHF_WRITE_ALLOC,
            0,
            0,
            section_align(".bss"),
            0,
            Vec::new(),
            bss_size,
        );
    }
    // Defined small data (`.sdata`/`.sbss`) precedes the read-only constant pool
    // (`.sdata2`), matching the section-name order above.
    if has_sdata {
        // `.sdata` holds the initialized values as file bytes.
        push(
            ".sdata",
            SHT_PROGBITS,
            SHF_WRITE_ALLOC,
            0,
            0,
            section_align(".sdata"),
            0,
            sdata,
            0,
        );
    }
    if has_sbss {
        // `.sbss` is NOBITS: no file bytes, but `sh_size` is the in-memory size.
        push(
            ".sbss",
            SHT_NOBITS,
            SHF_WRITE_ALLOC,
            0,
            0,
            section_align(".sbss"),
            0,
            Vec::new(),
            sbss_size,
        );
    }
    if has_constants || has_const_sdata2 {
        push(
            ".sdata2",
            SHT_PROGBITS,
            SHF_WRITE_ALLOC,
            0,
            0,
            section_align(".sdata2"),
            0,
            sdata2,
            0,
        );
    }
    if has_mwcats {
        push(
            &mwcats_section,
            SHT_MWCATS,
            0,
            index_of(text_section),
            0,
            4,
            1,
            mwcats,
            0,
        );
    }
    if has_text_relocations {
        push(
            &rela_text_section,
            SHT_RELA,
            0,
            symtab_section,
            index_of(text_section),
            4,
            12,
            rela_text,
            0,
        );
    }
    if has_frame {
        push(
            ".relaextabindex",
            SHT_RELA,
            0,
            symtab_section,
            index_of("extabindex"),
            4,
            12,
            rela_extabindex,
            0,
        );
    }
    if has_jump_table || has_data_relocs {
        push(
            ".rela.data",
            SHT_RELA,
            0,
            symtab_section,
            index_of(".data"),
            4,
            12,
            rela_data,
            0,
        );
    }
    if has_sdata_relocs {
        push(
            ".rela.sdata",
            SHT_RELA,
            0,
            symtab_section,
            index_of(".sdata"),
            4,
            12,
            rela_sdata,
            0,
        );
    }
    // Push order MUST match the `order` vector: `.rela.dtors` (early target) before
    // `.rela.sdata2` (late target). They are mutually exclusive in practice, but keep
    // the invariant so a future TU carrying both lays out correctly.
    if has_ctors_relocs {
        push(
            ".rela.ctors",
            SHT_RELA,
            0,
            symtab_section,
            index_of(".ctors"),
            4,
            12,
            rela_ctors,
            0,
        );
    }
    if has_dtors_relocs {
        push(
            ".rela.dtors",
            SHT_RELA,
            0,
            symtab_section,
            index_of(".dtors"),
            4,
            12,
            rela_dtors,
            0,
        );
    }
    if has_sdata2_relocs {
        push(
            ".rela.sdata2",
            SHT_RELA,
            0,
            symtab_section,
            index_of(".sdata2"),
            4,
            12,
            rela_sdata2,
            0,
        );
    }
    if has_mwcats {
        push(
            &rela_mwcats_section,
            SHT_RELA,
            0,
            symtab_section,
            index_of(&mwcats_section),
            4,
            12,
            rela_mwcats,
            0,
        );
    }
    push(
        ".symtab",
        SHT_SYMTAB,
        0,
        index_of(".strtab"),
        first_global_index,
        4,
        16,
        symtab,
        0,
    );
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
    write_elf_header(
        &mut output,
        section_headers_offset,
        sections.len() as u16,
        index_of(".shstrtab") as u16,
    );
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
            &mut output,
            section.name_offset,
            section.sh_type,
            section.flags,
            section_offset,
            section.size,
            section.link,
            section.info,
            section.align,
            section.entry_size,
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

fn write_elf_header(
    output: &mut Vec<u8>,
    section_headers_offset: u32,
    section_count: u16,
    shstrndx: u16,
) {
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

fn write_symbol(
    output: &mut Vec<u8>,
    name: u32,
    value: u32,
    size: u32,
    info: u8,
    other: u8,
    section_index: u16,
) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_comment_format_is_independent_of_compiler_identity() {
        let record = comment_record(
            CommentFormat {
                marker: 0x08,
                version: (2, 3, 0),
            },
            &[],
        );
        assert_eq!(record[11], 0x08);
        assert_eq!(&record[12..16], &[2, 3, 0, 1]);
    }
}
