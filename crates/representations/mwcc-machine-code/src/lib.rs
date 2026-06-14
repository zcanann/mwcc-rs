//! The machine-code representation: a sequence of PowerPC (Gekko) instructions
//! with their encodings.
//!
//! Instructions are structured (not raw words) so the register allocator and
//! instruction scheduler — the phases where byte-matching is won — can inspect
//! and rewrite them before the final encoding. Encodings are verified against
//! real `mwcceppc` output.

/// A PowerPC instruction in the v0 subset. Register fields are physical numbers;
/// virtual registers and a spill model arrive with the allocator (roadmap M1).
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// `addi rD, rA, SIMM` — also spells `li rD, SIMM` when `a == 0`.
    AddImmediate { d: u8, a: u8, immediate: i16 },
    /// `addis rD, rA, SIMM` — also spells `lis rD, SIMM` when `a == 0`.
    AddImmediateShifted { d: u8, a: u8, immediate: i16 },
    /// `ori rA, rS, UIMM`
    OrImmediate { a: u8, s: u8, immediate: u16 },
    /// `add rD, rA, rB`
    Add { d: u8, a: u8, b: u8 },
    /// `subf rD, rA, rB` => rD = rB - rA.
    SubtractFrom { d: u8, a: u8, b: u8 },
    /// `mullw rD, rA, rB`
    MultiplyLow { d: u8, a: u8, b: u8 },
    /// `mulli rD, rA, SIMM`
    MultiplyImmediate { d: u8, a: u8, immediate: i16 },
    /// `slwi rA, rS, shift` — shift left by `shift` (1..=31), via `rlwinm`.
    ShiftLeftImmediate { a: u8, s: u8, shift: u8 },
    /// `or rA, rS, rB` — spells `mr rA, rS` when `s == b`.
    Or { a: u8, s: u8, b: u8 },
    /// `and rA, rS, rB`
    And { a: u8, s: u8, b: u8 },
    /// `xor rA, rS, rB`
    Xor { a: u8, s: u8, b: u8 },
    /// `slw rA, rS, rB` — shift left word by the low bits of rB.
    ShiftLeftWord { a: u8, s: u8, b: u8 },
    /// `xori rA, rS, UIMM`
    XorImmediate { a: u8, s: u8, immediate: u16 },
    /// `clrlwi rA, rS, n` — clear the high `n` bits (mask to the low `32-n`), via `rlwinm`.
    ClearLeftImmediate { a: u8, s: u8, clear: u8 },
    /// `fadds frD, frA, frB`
    FloatAddSingle { d: u8, a: u8, b: u8 },
    /// `fsubs frD, frA, frB`
    FloatSubtractSingle { d: u8, a: u8, b: u8 },
    /// `fmuls frD, frA, frC`
    FloatMultiplySingle { d: u8, a: u8, c: u8 },
    /// `fmadds frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fmsubs frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fnmsubs frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fmr frD, frB`
    FloatMove { d: u8, b: u8 },
    /// `blr` — return to link register.
    BranchToLinkRegister,
}

impl Instruction {
    /// `li rD, SIMM`
    pub fn load_immediate(d: u8, immediate: i16) -> Self {
        Instruction::AddImmediate { d, a: 0, immediate }
    }
    /// `lis rD, SIMM`
    pub fn load_immediate_shifted(d: u8, immediate: i16) -> Self {
        Instruction::AddImmediateShifted { d, a: 0, immediate }
    }
    /// `mr rA, rS`
    pub fn move_register(a: u8, s: u8) -> Self {
        Instruction::Or { a, s, b: s }
    }

    /// Encode to a 32-bit big-endian instruction word.
    pub fn encode(&self) -> u32 {
        match *self {
            Instruction::AddImmediate { d, a, immediate } => d_form(14, d, a, immediate as u16),
            Instruction::AddImmediateShifted { d, a, immediate } => d_form(15, d, a, immediate as u16),
            Instruction::OrImmediate { a, s, immediate } => d_form(24, s, a, immediate),
            Instruction::Add { d, a, b } => xo_form(d, a, b, 266),
            Instruction::SubtractFrom { d, a, b } => xo_form(d, a, b, 40),
            Instruction::MultiplyLow { d, a, b } => xo_form(d, a, b, 235),
            Instruction::MultiplyImmediate { d, a, immediate } => d_form(7, d, a, immediate as u16),
            // slwi rA,rS,n == rlwinm rA,rS,n,0,31-n
            Instruction::ShiftLeftImmediate { a, s, shift } => {
                let mask_end = 31 - shift as u32;
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (mask_end << 1)
            }
            Instruction::Or { a, s, b } => logical_form(s, a, b, 444),
            Instruction::And { a, s, b } => logical_form(s, a, b, 28),
            Instruction::Xor { a, s, b } => logical_form(s, a, b, 316),
            Instruction::ShiftLeftWord { a, s, b } => logical_form(s, a, b, 24),
            Instruction::XorImmediate { a, s, immediate } => d_form(26, s, a, immediate),
            // clrlwi rA,rS,n == rlwinm rA,rS,0,n,31
            Instruction::ClearLeftImmediate { a, s, clear } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((clear as u32) << 6) | (31 << 1)
            }
            Instruction::FloatAddSingle { d, a, b } => a_form(59, d, a, b, 0, 21),
            Instruction::FloatSubtractSingle { d, a, b } => a_form(59, d, a, b, 0, 20),
            Instruction::FloatMultiplySingle { d, a, c } => a_form(59, d, a, 0, c, 25),
            Instruction::FloatMultiplyAddSingle { d, a, c, b } => a_form(59, d, a, b, c, 29),
            Instruction::FloatMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 28),
            Instruction::FloatNegativeMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 30),
            Instruction::FloatMove { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (72 << 1),
            Instruction::BranchToLinkRegister => 0x4E80_0020,
        }
    }
}

fn d_form(opcode: u32, d: u8, a: u8, immediate: u16) -> u32 {
    (opcode << 26) | ((d as u32) << 21) | ((a as u32) << 16) | (immediate as u32)
}
fn xo_form(d: u8, a: u8, b: u8, extended_opcode: u32) -> u32 {
    (31 << 26) | ((d as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | (extended_opcode << 1)
}
/// Logical/shift register form: opcode 31, rS in the D slot, rA in the A slot, rB.
fn logical_form(s: u8, a: u8, b: u8, extended_opcode: u32) -> u32 {
    (31 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((b as u32) << 11) | (extended_opcode << 1)
}
fn a_form(opcode: u32, d: u8, a: u8, b: u8, c: u8, extended_opcode: u32) -> u32 {
    (opcode << 26)
        | ((d as u32) << 21)
        | ((a as u32) << 16)
        | ((b as u32) << 11)
        | ((c as u32) << 6)
        | (extended_opcode << 1)
}

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

    /// Encode the whole function to big-endian `.text` bytes.
    pub fn encode_text(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.instructions.len() * 4);
        for instruction in &self.instructions {
            bytes.extend_from_slice(&instruction.encode().to_be_bytes());
        }
        bytes
    }
}
