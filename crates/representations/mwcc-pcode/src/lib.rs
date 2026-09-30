//! PCode: a function as basic blocks of PowerPC instructions over virtual
//! registers, mirroring MWCC's own backend program form.
//!
//! MWCC lowers expression trees to PCode ("INITIAL CODE"), optimizes and
//! schedules it while registers are still virtual, colors the interference
//! graph, then lays out the frame and schedules the physical result. Register
//! numbering follows MWCC exactly: numbers below 32 name physical registers,
//! and virtual registers of each class count upward from 32 (`vr32` is the
//! first). The numbering is observable — coloring breaks ties by it — so
//! producers must allocate virtual registers in MWCC's order.
//!
//! Instructions reuse [`mwcc_machine_code::Instruction`]; its register fields
//! already carry MWCC numbers (`mwcc_vreg::VIRTUAL_BASE` is 32). Relocations
//! and source provenance travel with each instruction so reordering passes need
//! no index remapping.

use mwcc_machine_code::{RelocationKind, RelocationTarget};
use mwcc_machine_code::Instruction;

pub use mwcc_vreg::Class;

pub mod mnemonic;
pub mod opcodes;

/// First virtual register number of every class.
pub const FIRST_VIRTUAL: u32 = 32;

/// A register of one class in MWCC numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Register {
    pub class: Class,
    pub number: u32,
}

impl Register {
    pub const fn general(number: u32) -> Self {
        Register { class: Class::General, number }
    }

    pub const fn float(number: u32) -> Self {
        Register { class: Class::Float, number }
    }

    pub fn is_virtual(self) -> bool {
        self.number >= FIRST_VIRTUAL
    }
}

/// A relocation attached to the instruction it patches.
#[derive(Debug, Clone)]
pub struct AttachedRelocation {
    pub kind: RelocationKind,
    pub target: RelocationTarget,
}

/// MWCC instruction flags that change allocation (subset of the recovered
/// `PCodeInstructionFlags`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InstructionFlags {
    /// A register copy that must not be coalesced away.
    pub coalesce_disabled: bool,
    /// Never delete as dead code (volatile access, call, store, …).
    pub side_effect: bool,
    /// An in-place update that stays in the web of the value it reads.
    pub in_place: bool,
}

/// One PCode instruction.
#[derive(Debug, Clone)]
pub struct PInstr {
    pub instruction: Instruction,
    pub relocation: Option<AttachedRelocation>,
    /// Registers read but not named by the instruction encoding (a call's
    /// argument registers, `blr`'s result registers).
    pub implicit_uses: Vec<Register>,
    /// Registers written but not named by the encoding (a call's clobbers).
    pub implicit_defs: Vec<Register>,
    /// General registers that may not be colored r0 (D-form bases, `addi`
    /// sources): MWCC records this as an interference with r0.
    pub not_r0: Vec<u32>,
    pub flags: InstructionFlags,
    /// Source line of the statement this instruction was generated for.
    pub source_line: Option<u32>,
}

impl PInstr {
    pub fn new(instruction: Instruction) -> Self {
        PInstr {
            instruction,
            relocation: None,
            implicit_uses: Vec::new(),
            implicit_defs: Vec::new(),
            not_r0: Vec::new(),
            flags: InstructionFlags::default(),
            source_line: None,
        }
    }

    /// Registers of `class` this instruction reads, explicit then implicit.
    pub fn uses(&self, class: Class) -> Vec<u32> {
        let mut registers: Vec<u32> = mwcc_vreg::register_operands(&self.instruction)
            .into_iter()
            .filter(|operand| {
                operand.class == class && operand.role == mwcc_vreg::RegisterRole::Use
            })
            .map(|operand| operand.register)
            .collect();
        registers.extend(
            self.implicit_uses
                .iter()
                .filter(|register| register.class == class)
                .map(|register| register.number),
        );
        registers
    }

    /// Registers of `class` this instruction writes, explicit then implicit.
    pub fn defs(&self, class: Class) -> Vec<u32> {
        let mut registers: Vec<u32> = mwcc_vreg::register_operands(&self.instruction)
            .into_iter()
            .filter(|operand| {
                operand.class == class
                    && operand.role == mwcc_vreg::RegisterRole::Define
            })
            .map(|operand| operand.register)
            .collect();
        registers.extend(
            self.implicit_defs
                .iter()
                .filter(|register| register.class == class)
                .map(|register| register.number),
        );
        registers
    }

    /// A register-to-register copy of `class` (`mr` / `fmr`): `(dest, src)`.
    pub fn copy(&self, class: Class) -> Option<(u32, u32)> {
        match (class, &self.instruction) {
            (Class::General, Instruction::Or { a, s, b }) if s == b => Some((*a, *s)),
            (Class::Float, Instruction::FloatMove { d, b }) => Some((*d, *b)),
            _ => None,
        }
    }

    /// Whether dead-code elimination must keep this instruction.
    pub fn has_side_effects(&self) -> bool {
        self.flags.side_effect
            || self.instruction.is_call()
            // Emitted only for the carry they set, which liveness does not track.
            || matches!(
                self.instruction,
                Instruction::SubtractFromCarrying { .. } | Instruction::AddCarrying { .. }
            )
            || is_memory_write_or_control(&self.instruction)
    }
}

fn is_memory_write_or_control(instruction: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        instruction,
        StoreWord { .. }
            | StoreByte { .. }
            | StoreHalfword { .. }
            | StoreFloatSingle { .. }
            | StoreFloatDouble { .. }
            | StoreWordWithUpdate { .. }
            | StoreWordIndexed { .. }
            | StoreByteIndexed { .. }
            | StoreHalfwordIndexed { .. }
            | StoreFloatSingleIndexed { .. }
            | StoreMultipleWord { .. }
            | BranchConditionalForward { .. }
            | BranchConditionalToLinkRegister { .. }
            | Branch { .. }
            | BranchExternal { .. }
            | BranchToLinkRegister
            | BranchToCountRegister
            | MoveToLinkRegister { .. }
            | MoveToCountRegister { .. }
            | InstructionSynchronize
            | Synchronize
            | EnforceInOrderIo
            | VerbatimWord(_)
    )
}

/// A basic block. Blocks are kept in layout order.
#[derive(Debug, Clone, Default)]
pub struct Block {
    pub instructions: Vec<PInstr>,
    /// Indices of successor blocks.
    pub successors: Vec<usize>,
    /// Loop-depth-derived execution weight (spill costs).
    pub weight: u32,
}

/// Registers the function's result occupies at return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnRegisters {
    None,
    General,
    GeneralPair,
    Float,
}

/// A function in PCode form.
#[derive(Debug, Clone)]
pub struct PCodeFunction {
    pub name: String,
    pub blocks: Vec<Block>,
    /// Next unallocated virtual register per class (starts at 32).
    pub next_general: u32,
    pub next_float: u32,
    /// First virtual register of each class that may coalesce with another
    /// virtual register (MWCC's `Registers_BeginCoalesceWindow`). Webs numbered
    /// before it — parameters and declared locals — only coalesce with
    /// physical registers.
    pub coalesce_first_general: u32,
    pub coalesce_first_float: u32,
    /// Registers numbered past the window that still stand for a
    /// pre-window web (split from a parameter or declared local): they, too,
    /// only coalesce with physical registers.
    pub outside_window: Vec<(Class, u32)>,
    /// Bytes of the frame's local area (stack homes at r1+8 upward).
    pub frame_local_bytes: i16,
    /// r1-relative [start, end) ranges of frame objects (locals and
    /// conversion slots): the scheduler orders accesses per object.
    pub frame_objects: Vec<(i16, i16)>,
    /// Starts of frame objects whose address never escapes: pointer-based
    /// accesses cannot reach them.
    pub private_frame_objects: Vec<i16>,
    /// Switch jump tables: the target block of each index, and the table
    /// symbol's offset past the function's anonymous-label counter.
    pub jump_tables: Vec<(Vec<usize>, u32)>,
    /// String literals the code addresses as `@@strN` (resolved per unit).
    pub strings: Vec<Vec<u8>>,
    /// General registers live at the exit besides the result (`-O0` keeps
    /// register variables live through the whole function).
    pub exit_uses: Vec<u32>,
    /// FPRs live everywhere (`-O0` floating register variables).
    pub exit_float_uses: Vec<u32>,
    /// The body ends in a sibling branch (`b callee`): no return of its own.
    pub ends_in_tail_call: bool,
    /// Pooled constants (bits, byte width) referenced as
    /// `RelocationTarget::Constant(index)`.
    pub pool: Vec<(u64, u8)>,
    pub returns: ReturnRegisters,
}

impl PCodeFunction {
    pub fn new(name: impl Into<String>, returns: ReturnRegisters) -> Self {
        PCodeFunction {
            name: name.into(),
            blocks: vec![Block { weight: 1, ..Block::default() }],
            next_general: FIRST_VIRTUAL,
            next_float: FIRST_VIRTUAL,
            coalesce_first_general: FIRST_VIRTUAL,
            coalesce_first_float: FIRST_VIRTUAL,
            outside_window: Vec::new(),
            frame_local_bytes: 0,
            frame_objects: Vec::new(),
            private_frame_objects: Vec::new(),
            jump_tables: Vec::new(),
            strings: Vec::new(),
            exit_uses: Vec::new(),
            exit_float_uses: Vec::new(),
            ends_in_tail_call: false,
            pool: Vec::new(),
            returns,
        }
    }

    /// Allocate the next virtual register of `class`, in MWCC numbering order.
    pub fn fresh(&mut self, class: Class) -> u32 {
        let counter = match class {
            Class::General => &mut self.next_general,
            Class::Float => &mut self.next_float,
        };
        let number = *counter;
        *counter += 1;
        number
    }

    /// Open the coalescing window at the current counters.
    pub fn begin_coalesce_window(&mut self) {
        self.coalesce_first_general = self.next_general;
        self.coalesce_first_float = self.next_float;
    }

    /// Whether virtual `register` may coalesce with another virtual register.
    pub fn in_coalesce_window(&self, class: Class, register: u32) -> bool {
        register >= self.coalesce_first(class) && !self.outside_window.contains(&(class, register))
    }

    pub fn coalesce_first(&self, class: Class) -> u32 {
        match class {
            Class::General => self.coalesce_first_general,
            Class::Float => self.coalesce_first_float,
        }
    }

    /// Register count of `class` (physical plus virtual), as coloring sizes it.
    pub fn register_count(&self, class: Class) -> u32 {
        match class {
            Class::General => self.next_general,
            Class::Float => self.next_float,
        }
    }

    pub fn instructions(&self) -> impl Iterator<Item = &PInstr> {
        self.blocks.iter().flat_map(|block| block.instructions.iter())
    }
}
