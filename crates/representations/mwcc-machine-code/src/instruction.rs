//! The `Instruction` representation: structured PowerPC (Gekko) instructions the
//! allocator and scheduler can inspect and rewrite before encoding.

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
    /// `extsb rA, rS` — sign-extend byte.
    ExtendSignByte { a: u8, s: u8 },
    /// `extsh rA, rS` — sign-extend halfword.
    ExtendSignHalfword { a: u8, s: u8 },
    /// `andc rA, rS, rB` => rA = rS & ~rB.
    AndComplement { a: u8, s: u8, b: u8 },
    /// `orc rA, rS, rB` => rA = rS | ~rB.
    OrComplement { a: u8, s: u8, b: u8 },
    /// `subfic rD, rA, SIMM` => rD = SIMM - rA.
    SubtractFromImmediate { d: u8, a: u8, immediate: i16 },
    /// `subfc rD, rA, rB` => rD = rB - rA, setting the carry.
    SubtractFromCarrying { d: u8, a: u8, b: u8 },
    /// `adde rD, rA, rB` => rD = rA + rB + carry.
    AddExtended { d: u8, a: u8, b: u8 },
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
    /// `rlwinm rA, rS, 0, begin, end` — keep the contiguous bit run `[begin, end]`.
    AndContiguousMask { a: u8, s: u8, begin: u8, end: u8 },
    /// `rlwinm rA, rS, shift, begin, end` — rotate left by `shift`, keep bits
    /// `[begin, end]`. The general form; mwcc fuses a narrow unsigned shift and
    /// its width mask into one of these.
    RotateAndMask { a: u8, s: u8, shift: u8, begin: u8, end: u8 },
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
    /// `lbz rD, offset(rA)` — load byte, zero-extended.
    LoadByteZero { d: u8, a: u8, offset: i16 },
    /// `lhz rD, offset(rA)` — load halfword, zero-extended.
    LoadHalfwordZero { d: u8, a: u8, offset: i16 },
    /// `lha rD, offset(rA)` — load halfword, sign-extended.
    LoadHalfwordAlgebraic { d: u8, a: u8, offset: i16 },
    /// `lfs frD, offset(rA)` — load float single.
    LoadFloatSingle { d: u8, a: u8, offset: i16 },
    /// `stfd frS, offset(rA)` — store float double.
    StoreFloatDouble { s: u8, a: u8, offset: i16 },
    /// `fcmpo crf0, frA, frB` — ordered float compare.
    FloatCompareOrdered { a: u8, b: u8 },
    /// `cmpwi crf0, rA, SIMM` — signed compare against an immediate.
    CompareWordImmediate { a: u8, immediate: i16 },
    /// `cmpw crf0, rA, rB` — signed compare.
    CompareWord { a: u8, b: u8 },
    /// `cmplwi crf0, rA, UIMM` — unsigned compare against an immediate.
    CompareLogicalWordImmediate { a: u8, immediate: u16 },
    /// `cmplw crf0, rA, rB` — unsigned compare.
    CompareLogicalWord { a: u8, b: u8 },
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
}
