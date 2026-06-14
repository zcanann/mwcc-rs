//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object matching mwcceppc's layout (sections, symbols,
//! relocations, and the Metrowerks metadata records).

use mwcc_machine_code::MachineFunction;
use mwcc_object::{ObjectInput, TextRelocation};

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
            offset: relocation.instruction_index as u32 * 4,
            elf_type: relocation.kind.elf_type(),
            symbol: relocation.symbol.clone(),
        })
        .collect();
    mwcc_object::write_object(&ObjectInput {
        source_name,
        function_name: &function.name,
        text: &text,
        version,
        build,
        relocations,
    })
}
