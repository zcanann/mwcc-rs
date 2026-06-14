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
    /// `neg rD, rA`
    Negate { d: u8, a: u8 },
    /// `nor rA, rS, rB` — spells `not rA, rS` when `s == b`.
    Nor { a: u8, s: u8, b: u8 },
    /// `cntlzw rA, rS` — count leading zero bits.
    CountLeadingZeros { a: u8, s: u8 },
    /// `andc rA, rS, rB` => rA = rS & ~rB.
    AndComplement { a: u8, s: u8, b: u8 },
    /// `subfic rD, rA, SIMM` => rD = SIMM - rA.
    SubtractFromImmediate { d: u8, a: u8, immediate: i16 },
    /// `mullw rD, rA, rB`
    MultiplyLow { d: u8, a: u8, b: u8 },
    /// `mulli rD, rA, SIMM`
    MultiplyImmediate { d: u8, a: u8, immediate: i16 },
    /// `divw rD, rA, rB` — signed divide.
    DivideWord { d: u8, a: u8, b: u8 },
    /// `divwu rD, rA, rB` — unsigned divide.
    DivideWordUnsigned { d: u8, a: u8, b: u8 },
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
    /// `sraw rA, rS, rB` — arithmetic (signed) shift right word.
    ShiftRightAlgebraicWord { a: u8, s: u8, b: u8 },
    /// `srw rA, rS, rB` — logical (unsigned) shift right word.
    ShiftRightWord { a: u8, s: u8, b: u8 },
    /// `srawi rA, rS, shift` — arithmetic shift right immediate.
    ShiftRightAlgebraicImmediate { a: u8, s: u8, shift: u8 },
    /// `srwi rA, rS, shift` — logical shift right immediate, via `rlwinm`.
    ShiftRightLogicalImmediate { a: u8, s: u8, shift: u8 },
    /// `xori rA, rS, UIMM`
    XorImmediate { a: u8, s: u8, immediate: u16 },
    /// `xoris rA, rS, UIMM`
    XorImmediateShifted { a: u8, s: u8, immediate: u16 },
    /// `stw rS, offset(rA)` — store word.
    StoreWord { s: u8, a: u8, offset: i16 },
    /// `lfd frD, offset(rA)` — load float double.
    LoadFloatDouble { d: u8, a: u8, offset: i16 },
    /// `clrlwi rA, rS, n` — clear the high `n` bits (mask to the low `32-n`), via `rlwinm`.
    ClearLeftImmediate { a: u8, s: u8, clear: u8 },
    /// `fadds frD, frA, frB`
    FloatAddSingle { d: u8, a: u8, b: u8 },
    /// `fsubs frD, frA, frB`
    FloatSubtractSingle { d: u8, a: u8, b: u8 },
    /// `fmuls frD, frA, frC`
    FloatMultiplySingle { d: u8, a: u8, c: u8 },
    /// `fdivs frD, frA, frB`
    FloatDivideSingle { d: u8, a: u8, b: u8 },
    /// `fmadds frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fmsubs frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fnmsubs frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fmr frD, frB`
    FloatMove { d: u8, b: u8 },
    /// `fneg frD, frB`
    FloatNegate { d: u8, b: u8 },
    /// `fctiwz frD, frB` — convert float to integer, round toward zero.
    ConvertToIntegerWordZero { d: u8, b: u8 },
    /// `stwu rS, offset(rA)` — store word with base update (stack frame push).
    StoreWordWithUpdate { s: u8, a: u8, offset: i16 },
    /// `lwz rD, offset(rA)` — load word.
    LoadWord { d: u8, a: u8, offset: i16 },
    /// `stfd frS, offset(rA)` — store float double.
    StoreFloatDouble { s: u8, a: u8, offset: i16 },
    /// `cmpwi crf0, rA, SIMM` — signed compare against an immediate.
    CompareWordImmediate { a: u8, immediate: i16 },
    /// `cmpw crf0, rA, rB` — signed compare.
    CompareWord { a: u8, b: u8 },
    /// A forward conditional branch to another instruction (by index). `options`
    /// is the PowerPC BO field, `condition_bit` the BI field (cr0: 0=LT,1=GT,2=EQ).
    /// The byte offset is resolved at encode time from the instruction positions.
    BranchConditionalForward { options: u8, condition_bit: u8, target: usize },
    /// `bclr BO, BI` — conditional return (e.g. `bnelr`).
    BranchConditionalToLinkRegister { options: u8, condition_bit: u8 },
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
            Instruction::Negate { d, a } => xo_form(d, a, 0, 104),
            Instruction::Nor { a, s, b } => logical_form(s, a, b, 124),
            Instruction::CountLeadingZeros { a, s } => logical_form(s, a, 0, 26),
            Instruction::AndComplement { a, s, b } => logical_form(s, a, b, 60),
            Instruction::SubtractFromImmediate { d, a, immediate } => d_form(8, d, a, immediate as u16),
            Instruction::MultiplyLow { d, a, b } => xo_form(d, a, b, 235),
            Instruction::MultiplyImmediate { d, a, immediate } => d_form(7, d, a, immediate as u16),
            Instruction::DivideWord { d, a, b } => xo_form(d, a, b, 491),
            Instruction::DivideWordUnsigned { d, a, b } => xo_form(d, a, b, 459),
            // slwi rA,rS,n == rlwinm rA,rS,n,0,31-n
            Instruction::ShiftLeftImmediate { a, s, shift } => {
                let mask_end = 31 - shift as u32;
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (mask_end << 1)
            }
            Instruction::Or { a, s, b } => logical_form(s, a, b, 444),
            Instruction::And { a, s, b } => logical_form(s, a, b, 28),
            Instruction::Xor { a, s, b } => logical_form(s, a, b, 316),
            Instruction::ShiftLeftWord { a, s, b } => logical_form(s, a, b, 24),
            Instruction::ShiftRightAlgebraicWord { a, s, b } => logical_form(s, a, b, 792),
            Instruction::ShiftRightWord { a, s, b } => logical_form(s, a, b, 536),
            Instruction::ShiftRightAlgebraicImmediate { a, s, shift } => {
                (31 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((shift as u32) << 11) | (824 << 1)
            }
            // srwi rA,rS,n == rlwinm rA,rS,32-n,n,31
            Instruction::ShiftRightLogicalImmediate { a, s, shift } => {
                let rotate = 32 - shift as u32;
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | (rotate << 11) | ((shift as u32) << 6) | (31 << 1)
            }
            Instruction::XorImmediate { a, s, immediate } => d_form(26, s, a, immediate),
            Instruction::XorImmediateShifted { a, s, immediate } => d_form(27, s, a, immediate),
            Instruction::StoreWord { s, a, offset } => d_form(36, s, a, offset as u16),
            Instruction::LoadFloatDouble { d, a, offset } => d_form(50, d, a, offset as u16),
            // clrlwi rA,rS,n == rlwinm rA,rS,0,n,31
            Instruction::ClearLeftImmediate { a, s, clear } => {
                (21 << 26) | ((s as u32) << 21) | ((a as u32) << 16) | ((clear as u32) << 6) | (31 << 1)
            }
            Instruction::FloatAddSingle { d, a, b } => a_form(59, d, a, b, 0, 21),
            Instruction::FloatSubtractSingle { d, a, b } => a_form(59, d, a, b, 0, 20),
            Instruction::FloatMultiplySingle { d, a, c } => a_form(59, d, a, 0, c, 25),
            Instruction::FloatDivideSingle { d, a, b } => a_form(59, d, a, b, 0, 18),
            Instruction::FloatMultiplyAddSingle { d, a, c, b } => a_form(59, d, a, b, c, 29),
            Instruction::FloatMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 28),
            Instruction::FloatNegativeMultiplySubtractSingle { d, a, c, b } => a_form(59, d, a, b, c, 30),
            Instruction::FloatMove { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (72 << 1),
            Instruction::FloatNegate { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (40 << 1),
            Instruction::ConvertToIntegerWordZero { d, b } => (63 << 26) | ((d as u32) << 21) | ((b as u32) << 11) | (15 << 1),
            Instruction::StoreWordWithUpdate { s, a, offset } => d_form(37, s, a, offset as u16),
            Instruction::LoadWord { d, a, offset } => d_form(32, d, a, offset as u16),
            Instruction::StoreFloatDouble { s, a, offset } => d_form(54, s, a, offset as u16),
            Instruction::CompareWordImmediate { a, immediate } => (11 << 26) | ((a as u32) << 16) | (immediate as u16 as u32),
            Instruction::CompareWord { a, b } => (31 << 26) | ((a as u32) << 16) | ((b as u32) << 11),
            // resolved positionally in encode_text
            Instruction::BranchConditionalForward { .. } => 0,
            Instruction::BranchConditionalToLinkRegister { options, condition_bit } => {
                (19 << 26) | ((options as u32) << 21) | ((condition_bit as u32) << 16) | (16 << 1)
            }
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
