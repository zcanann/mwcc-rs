//! Pipeline: machine code -> relocatable object file.
//!
//! Encodes the function's instructions to `.text` and wraps them in an ELF32
//! big-endian PowerPC object. v0 emits one function per object; sections, data,
//! and relocations grow with the language.

use mwcc_machine_code::MachineFunction;
use mwcc_object::DefinedFunction;

pub fn assemble_object(function: &MachineFunction) -> Vec<u8> {
    let text = function.encode_text();
    mwcc_object::write_single_function(&DefinedFunction { name: &function.name, text: &text })
}
