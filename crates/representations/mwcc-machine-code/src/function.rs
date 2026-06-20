//! A function's worth of machine code, and its lowering to `.text` bytes.

use crate::frame::FrameInfo;
use crate::instruction::Instruction;
use crate::relocation::Relocation;

/// A read-only constant in the `.sdata2` pool: its big-endian bit pattern and
/// byte width (4 for a single-precision float, 8 for the int->float bias double).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolConstant {
    pub bits: u64,
    pub byte_width: u8,
}

/// A dense `switch`'s jump table (the writer materializes it as an anonymous `@N`
/// object in `.data`).
#[derive(Debug, Clone)]
pub struct JumpTable {
    /// One `.text` byte offset (within the function) per index — the body the
    /// dispatch branches to (gaps point at the default body).
    pub entries: Vec<u32>,
    /// How far past the function's running anonymous-`@N` counter the table's
    /// symbol sits: a label per case plus the dispatch, and one more for an
    /// explicit `default:` label.
    pub anonymous_offset: u32,
}

/// A function's worth of machine code.
#[derive(Debug, Clone, Default)]
pub struct MachineFunction {
    pub name: String,
    pub instructions: Vec<Instruction>,
    /// `.text` relocations, by the instruction they patch.
    pub relocations: Vec<Relocation>,
    /// Read-only constants this function loads from `.sdata2`. Each becomes an
    /// anonymous `@N` object that the function's `R_PPC_EMB_SDA21` loads reference.
    pub constants: Vec<PoolConstant>,
    /// Whether the function performs an int<->float conversion. mwcc's anonymous
    /// `@N` counter starts one higher for such functions.
    pub has_conversion: bool,
    /// Whether the function emits a floating-point conditional branch. mwcc's
    /// anonymous `@N` counter advances by three for such a branch.
    pub has_float_branch: bool,
    /// Extra `@N`-counter advance from the function's internal control-flow labels:
    /// mwcc numbers the basic-block labels an `if`/loop introduces before the
    /// function's unwind `@N` entries. Measured per construct — a non-leaf `if` adds
    /// 2, a `do … while` loop adds 6.
    pub anonymous_label_bump: u32,
    /// Frame metadata for the unwind tables; `None` for a leaf with no frame.
    pub frame: Option<FrameInfo>,
    /// A dense `switch`'s jump table; `None` unless the function dispatches through
    /// one. The writer materializes it as an anonymous `@N` object in `.data`.
    pub jump_table: Option<JumpTable>,
    /// Referenced symbol names (globals and callees) in mwcc's symbol-table order —
    /// an AST traversal, NOT `.text` reference order. The writer assigns this
    /// function's external/global symbol indices in this order (see the codegen's
    /// `symbol_order`), falling back to relocation order for anything not listed.
    pub symbol_order: Vec<String>,
}

impl MachineFunction {
    pub fn new(name: impl Into<String>) -> Self {
        MachineFunction {
            name: name.into(),
            instructions: Vec::new(),
            relocations: Vec::new(),
            constants: Vec::new(),
            has_conversion: false,
            has_float_branch: false,
            anonymous_label_bump: 0,
            frame: None,
            jump_table: None,
            symbol_order: Vec::new(),
        }
    }

    /// Intern a pool constant, returning its index. Equal constants share one slot
    /// (mwcc pools identical constants).
    pub fn intern_constant(&mut self, bits: u64, byte_width: u8) -> usize {
        let constant = PoolConstant { bits, byte_width };
        if let Some(index) = self.constants.iter().position(|existing| *existing == constant) {
            return index;
        }
        self.constants.push(constant);
        self.constants.len() - 1
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
                Instruction::Branch { target } => {
                    let offset = (target as i64 - index as i64) * 4;
                    (18 << 26) | ((offset as u32) & 0x03ff_fffc)
                }
                ref other => other.encode(),
            };
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        bytes
    }
}
