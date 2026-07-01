//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object matching mwcceppc's layout (sections, symbols,
//! relocations, and the Metrowerks metadata records).

use mwcc_machine_code::{MachineFunction, RelocationTarget as MachineTarget};
use mwcc_object::{DataObject, FrameLayout, FunctionObject, JumpTable, ObjectInput, RelocationTarget, Sdata2Constant, TextRelocation};

/// A data-section `ADDR32` relocation (re-exported so callers can build a
/// `DefinedGlobal`'s relocations without depending on `mwcc-object` directly).
pub use mwcc_object::DataRelocation;

/// A file-scope variable *defined* in this unit (placed in a data section), in
/// declaration order. The caller decides which globals qualify (non-`extern`,
/// laid out); the object writer assigns their section offsets and symbols.
pub struct DefinedGlobal {
    pub name: String,
    pub size: u32,
    pub alignment: u32,
    pub initial_bytes: Option<Vec<u8>>,
    /// A `const` global is read-only: the writer routes it to `.sdata2` (≤ 8
    /// bytes) or `.rodata` (larger) rather than `.sdata`/`.sbss`.
    pub is_const: bool,
    /// A `static` global binds as a LOCAL symbol (file-scope, not exported).
    pub is_static: bool,
    /// `ADDR32` data relocations the global's bytes carry (a pointer to a symbol).
    pub relocations: Vec<mwcc_object::DataRelocation>,
}

/// Assemble a relocatable object from one or more lowered functions (in source
/// order) plus the file-scope variables defined in the unit. `source_name` is the
/// source file's base name (e.g. "foo.c"), used for the object's `FILE` symbol;
/// `version` is the compiler version being reproduced, stamped into `.comment`.
/// The functions share one `.text`, one `.sdata2` constant pool, one
/// `.mwcats.text`, the unwind sections, and the `.sbss` data section.
pub fn assemble_object(functions: &[MachineFunction], defined_globals: &[DefinedGlobal], inline_asm_symbols: &[String], source_name: &str, version: (u8, u8, u8), build: u16, small_data: bool) -> Vec<u8> {
    // The encoded text is owned here so the borrowed `FunctionObject` can point at
    // it for the lifetime of the call.
    let texts: Vec<Vec<u8>> = functions.iter().map(|function| function.encode_text()).collect();
    let function_objects = functions
        .iter()
        .zip(&texts)
        .map(|(function, text)| FunctionObject {
            name: &function.name,
            is_static: function.is_static,
            text,
            // Each codegen relocation patches one instruction; its byte offset
            // (relative to the function) is four times the instruction index plus
            // the kind's field offset (the ADDR16 immediate sits in the low
            // halfword, at instruction+2).
            relocations: function
                .relocations
                .iter()
                .map(|relocation| TextRelocation {
                    offset: relocation.instruction_index as u32 * 4 + relocation.kind.field_offset(),
                    elf_type: relocation.kind.elf_type(),
                    target: match &relocation.target {
                        MachineTarget::External(symbol) => RelocationTarget::External(symbol.clone()),
                        MachineTarget::Constant(index) => RelocationTarget::Constant(*index),
                        MachineTarget::JumpTable => RelocationTarget::JumpTable,
                    },
                })
                .collect(),
            constants: function
                .constants
                .iter()
                .map(|constant| Sdata2Constant { bits: constant.bits, byte_width: constant.byte_width })
                .collect(),
            frame: function.frame.map(|frame| FrameLayout { extab_header: frame.extab_header() }),
            // The anonymous-`@N` counter is bumped by one for an int<->float
            // conversion and by three for a float conditional branch before this
            // function's constants are numbered.
            anonymous_bump: (if function.has_conversion { 1 } else { 0 })
                + (if function.has_float_branch { 3 } else { 0 })
                + function.anonymous_label_bump,
            // The unit's string resolver set these: the function's NEW-string count and the `@N`
            // names of those strings. The writer numbers/emits them at the front of the function's
            // `@N` block (interleaved per-function with its constants and unwind entries).
            string_count: function.new_string_count,
            string_names: function.new_string_names.clone(),
            jump_table: function.jump_table.as_ref().map(|table| JumpTable {
                entries: table.entries.clone(),
                anonymous_offset: table.anonymous_offset,
            }),
            symbol_order: function.symbol_order.clone(),
        })
        .collect();
    let data_objects = defined_globals
        .iter()
        .map(|global| DataObject { name: &global.name, size: global.size, alignment: global.alignment, initial_bytes: global.initial_bytes.clone(), is_const: global.is_const, is_static: global.is_static, relocations: global.relocations.clone() })
        .collect();
    mwcc_object::write_object(&ObjectInput { source_name, version, build, functions: function_objects, data_objects, small_data, inline_asm_symbols })
}
