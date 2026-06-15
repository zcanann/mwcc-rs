//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object matching mwcceppc's layout (sections, symbols,
//! relocations, and the Metrowerks metadata records).

use mwcc_machine_code::{MachineFunction, RelocationTarget as MachineTarget};
use mwcc_object::{FrameLayout, ObjectInput, RelocationTarget, Sdata2Constant, TextRelocation};

/// Assemble a relocatable object. `source_name` is the source file's base name
/// (e.g. "foo.c"), used for the object's `FILE` symbol; `version` is the compiler
/// version being reproduced, stamped into `.comment`.
pub fn assemble_object(function: &MachineFunction, source_name: &str, version: (u8, u8, u8), build: u16) -> Vec<u8> {
    let text = function.encode_text();
    // Each codegen relocation patches one instruction; its `.text` byte offset is
    // four times the instruction index.
    let relocations = function
        .relocations
        .iter()
        .map(|relocation| TextRelocation {
            // The instruction's byte offset plus the kind's field offset (the
            // ADDR16 immediate sits in the low halfword, at instruction+2).
            offset: relocation.instruction_index as u32 * 4 + relocation.kind.field_offset(),
            elf_type: relocation.kind.elf_type(),
            target: match &relocation.target {
                MachineTarget::External(symbol) => RelocationTarget::External(symbol.clone()),
                MachineTarget::Constant(index) => RelocationTarget::Constant(*index),
            },
        })
        .collect();
    let frame = function.frame.map(|frame| FrameLayout { extab_header: frame.extab_header() });
    mwcc_object::write_object(&ObjectInput {
        source_name,
        function_name: &function.name,
        text: &text,
        version,
        build,
        relocations,
        constants: function
            .constants
            .iter()
            .map(|constant| Sdata2Constant { bits: constant.bits, byte_width: constant.byte_width })
            .collect(),
        // mwcceppc's anonymous-symbol counter starts at 5, advancing by one for an
        // int<->float conversion and by three for a float conditional branch.
        anonymous_base: 5 + if function.has_conversion { 1 } else { 0 } + if function.has_float_branch { 3 } else { 0 },
        frame,
    })
}
