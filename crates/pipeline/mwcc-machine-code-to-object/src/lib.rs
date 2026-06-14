//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object matching mwcceppc's layout (sections, symbols,
//! relocations, and the Metrowerks metadata records).

use mwcc_machine_code::MachineFunction;
use mwcc_object::ObjectInput;

/// Assemble a relocatable object. `source_name` is the source file's base name
/// (e.g. "foo.c"), used for the object's `FILE` symbol.
pub fn assemble_object(function: &MachineFunction, source_name: &str) -> Vec<u8> {
    let text = function.encode_text();
    mwcc_object::write_object(&ObjectInput {
        source_name,
        function_name: &function.name,
        text: &text,
    })
}
