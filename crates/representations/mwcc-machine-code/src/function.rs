//! A function's worth of machine code, and its lowering to `.text` bytes.

use crate::instruction::Instruction;

/// A function's worth of machine code.
#[derive(Debug, Clone, Default)]
pub struct MachineFunction {
    pub name: String,
    pub instructions: Vec<Instruction>,
}

impl MachineFunction {
    pub fn new(name: impl Into<String>) -> Self {
        MachineFunction { name: name.into(), instructions: Vec::new() }
    }

    /// Encode the whole function to big-endian `.text` bytes. Forward conditional
    /// branches are resolved here from instruction positions.
    pub fn encode_text(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.instructions.len() * 4);
        for (index, instruction) in self.instructions.iter().enumerate() {
            let word = match *instruction {
                Instruction::BranchConditionalForward { options, condition_bit, target } => {
                    let offset = (target as i64 - index as i64) * 4;
                    (16 << 26) | ((options as u32) << 21) | ((condition_bit as u32) << 16) | ((offset as u32) & 0xfffc)
                }
                ref other => other.encode(),
            };
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes
    }
}
