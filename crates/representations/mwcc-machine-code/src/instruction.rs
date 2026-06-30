//! The `Instruction` representation: structured PowerPC (Gekko) instructions the
//! allocator and scheduler can inspect and rewrite before encoding.

/// A PowerPC instruction in the v0 subset. Register fields are physical numbers;
/// virtual registers and a spill model arrive with the allocator (roadmap M1).
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// `addi rD, rA, SIMM` — also spells `li rD, SIMM` when `a == 0`.
    AddImmediate { d: u8, a: u8, immediate: i16 },
    /// `addic. rD, rA, SIMM` — add immediate carrying, recording the result in CR0
    /// (so a following `bne`/`beq` tests it). Used by a loop counter decrement
    /// (`--n` → `addic. rN, rN, -1`).
    AddImmediateCarryingRecord { d: u8, a: u8, immediate: i16 },
    /// `addis rD, rA, SIMM` — also spells `lis rD, SIMM` when `a == 0`.
    AddImmediateShifted { d: u8, a: u8, immediate: i16 },
    /// `ori rA, rS, UIMM`
    OrImmediate { a: u8, s: u8, immediate: u16 },
    /// `oris rA, rS, UIMM` — OR the immediate into the high half.
    OrImmediateShifted { a: u8, s: u8, immediate: u16 },
    /// `add rD, rA, rB`
    Add { d: u8, a: u8, b: u8 },
    /// `subf rD, rA, rB` => rD = rB - rA.
    SubtractFrom { d: u8, a: u8, b: u8 },
    /// `neg rD, rA`
    Negate { d: u8, a: u8 },
    /// `nor rA, rS, rB` — spells `not rA, rS` when `s == b`.
    Nor { a: u8, s: u8, b: u8 },
    /// `nand rA, rS, rB` — `~(rS & rB)`.
    Nand { a: u8, s: u8, b: u8 },
    /// `eqv rA, rS, rB` — `~(rS ^ rB)`.
    Eqv { a: u8, s: u8, b: u8 },
    /// `cntlzw rA, rS` — count leading zero bits.
    CountLeadingZeros { a: u8, s: u8 },
    /// `extsb rA, rS` — sign-extend byte.
    ExtendSignByte { a: u8, s: u8 },
    /// `extsb. rA, rS` — sign-extend byte, record form (sets cr0). mwcc uses this to
    /// test a signed `char` condition: sign-extend and compare against 0 in one.
    ExtendSignByteRecord { a: u8, s: u8 },
    /// `extsh rA, rS` — sign-extend halfword.
    ExtendSignHalfword { a: u8, s: u8 },
    /// `extsh. rA, rS` — sign-extend halfword, record form (sets cr0). mwcc uses this
    /// to test a signed `short` against 0 in one instruction.
    ExtendSignHalfwordRecord { a: u8, s: u8 },
    /// `andc rA, rS, rB` => rA = rS & ~rB.
    AndComplement { a: u8, s: u8, b: u8 },
    /// `orc rA, rS, rB` => rA = rS | ~rB.
    OrComplement { a: u8, s: u8, b: u8 },
    /// `subfic rD, rA, SIMM` => rD = SIMM - rA.
    SubtractFromImmediate { d: u8, a: u8, immediate: i16 },
    /// `subfc rD, rA, rB` => rD = rB - rA, setting the carry.
    SubtractFromCarrying { d: u8, a: u8, b: u8 },
    /// `subfe rD, rA, rB` => rD = rB - rA + carry - 1 (the carrying high word of a 64-bit subtract).
    SubtractFromExtended { d: u8, a: u8, b: u8 },
    /// `addc rD, rA, rB` => rD = rA + rB, setting the carry (the low word of a 64-bit add).
    AddCarrying { d: u8, a: u8, b: u8 },
    /// `adde rD, rA, rB` => rD = rA + rB + carry.
    AddExtended { d: u8, a: u8, b: u8 },
    /// `addze rD, rA` => rD = rA + carry. Used to round a signed power-of-two
    /// division toward zero after an arithmetic shift.
    AddToZeroExtended { d: u8, a: u8 },
    /// `mullw rD, rA, rB`
    MultiplyLow { d: u8, a: u8, b: u8 },
    /// `mulhw rD, rA, rB` — high 32 bits of the signed product.
    MultiplyHighWord { d: u8, a: u8, b: u8 },
    /// `mulhwu rD, rA, rB` — high 32 bits of the unsigned product.
    MultiplyHighWordUnsigned { d: u8, a: u8, b: u8 },
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
    /// `stb rS, offset(rA)` — store byte.
    StoreByte { s: u8, a: u8, offset: i16 },
    /// `sth rS, offset(rA)` — store halfword.
    StoreHalfword { s: u8, a: u8, offset: i16 },
    /// `stfs frS, offset(rA)` — store float single.
    StoreFloatSingle { s: u8, a: u8, offset: i16 },
    /// `stwx rS, rA, rB` — store word indexed.
    StoreWordIndexed { s: u8, a: u8, b: u8 },
    /// `stbx rS, rA, rB` — store byte indexed.
    StoreByteIndexed { s: u8, a: u8, b: u8 },
    /// `sthx rS, rA, rB` — store halfword indexed.
    StoreHalfwordIndexed { s: u8, a: u8, b: u8 },
    /// `stfsx frS, rA, rB` — store float single indexed.
    StoreFloatSingleIndexed { s: u8, a: u8, b: u8 },
    /// `lfd frD, offset(rA)` — load float double.
    LoadFloatDouble { d: u8, a: u8, offset: i16 },
    /// `lfdx frD, rA, rB` — load float double indexed.
    LoadFloatDoubleIndexed { d: u8, a: u8, b: u8 },
    /// `stfdx frS, rA, rB` — store float double indexed.
    StoreFloatDoubleIndexed { s: u8, a: u8, b: u8 },
    /// `clrlwi rA, rS, n` — clear the high `n` bits (mask to the low `32-n`), via `rlwinm`.
    ClearLeftImmediate { a: u8, s: u8, clear: u8 },
    /// `clrlwi. rA, rS, n` — record form (`rlwinm.`): zero-extend a narrow unsigned
    /// value and set cr0, mwcc's one-instruction test of an unsigned `char`/`short`
    /// against 0.
    ClearLeftImmediateRecord { a: u8, s: u8, clear: u8 },
    /// `rlwinm rA, rS, 0, begin, end` — keep the contiguous bit run `[begin, end]`.
    AndContiguousMask { a: u8, s: u8, begin: u8, end: u8 },
    /// `rlwinm rA, rS, shift, begin, end` — rotate left by `shift`, keep bits
    /// `[begin, end]`. The general form; mwcc fuses a narrow unsigned shift and
    /// its width mask into one of these.
    RotateAndMask { a: u8, s: u8, shift: u8, begin: u8, end: u8 },
    /// `rlwnm rA, rS, rB, begin, end` — like `rlwinm` but the rotate amount is the
    /// low five bits of `rB` (a register) rather than an immediate. mwcc uses it for
    /// the `x <= 0` idiom: rotating a `1` left by `cntlzw(x)` lands in the low bit
    /// only when the leading-zero count is 0 or 32 (i.e. `x < 0` or `x == 0`).
    RotateAndMaskVariable { a: u8, s: u8, b: u8, begin: u8, end: u8 },
    /// `rlwimi rA, rS, shift, begin, end` — rotate `rS` left by `shift` and insert
    /// bits `[begin, end]` into `rA`, leaving `rA`'s other bits intact. mwcc uses
    /// it to merge two disjoint bit fields (e.g. an OR of two shifts, or a masked
    /// sign/magnitude merge) into one instruction.
    RotateAndMaskInsert { a: u8, s: u8, shift: u8, begin: u8, end: u8 },
    /// `rlwinm. rA, rS, 0, begin, end` — keep the bit run `[begin, end]` of `rS`
    /// and set cr0 from the result. Used to test `(x & mask)` in a condition.
    AndMaskRecord { a: u8, s: u8, begin: u8, end: u8 },
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
    /// Double-precision arithmetic (opcode 63 vs the single forms' 59).
    /// `fadd frD, frA, frB`
    FloatAddDouble { d: u8, a: u8, b: u8 },
    /// `fsub frD, frA, frB`
    FloatSubtractDouble { d: u8, a: u8, b: u8 },
    /// `fmul frD, frA, frC`
    FloatMultiplyDouble { d: u8, a: u8, c: u8 },
    /// `fmadd frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `fmsub frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `fnmsub frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `frsp frD, frB` — round a double to single precision.
    RoundToSingle { d: u8, b: u8 },
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
    /// `lwzx rD, rA, rB` — load word indexed.
    LoadWordIndexed { d: u8, a: u8, b: u8 },
    /// `lbzx rD, rA, rB` — load byte indexed, zero-extended.
    LoadByteZeroIndexed { d: u8, a: u8, b: u8 },
    /// `lhzx rD, rA, rB` — load halfword indexed, zero-extended.
    LoadHalfwordZeroIndexed { d: u8, a: u8, b: u8 },
    /// `lhax rD, rA, rB` — load halfword indexed, sign-extended.
    LoadHalfwordAlgebraicIndexed { d: u8, a: u8, b: u8 },
    /// `lfsx frD, rA, rB` — load float single indexed.
    LoadFloatSingleIndexed { d: u8, a: u8, b: u8 },
    /// `stfd frS, offset(rA)` — store float double.
    StoreFloatDouble { s: u8, a: u8, offset: i16 },
    /// `fcmpo crf0, frA, frB` — ordered float compare.
    FloatCompareOrdered { a: u8, b: u8 },
    /// `fcmpu crf0, frA, frB` — unordered float compare (mwcc uses this for `==`/`!=`).
    FloatCompareUnordered { a: u8, b: u8 },
    /// `mfcr rD` — move the whole condition register into a GPR.
    MoveFromConditionRegister { d: u8 },
    /// `cror crbD, crbA, crbB` — OR two condition-register bits into a third.
    /// Bit numbers are absolute (cr0 occupies bits 0..=3: lt=0, gt=1, eq=2, so=3).
    ConditionRegisterOr { d: u8, a: u8, b: u8 },
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
    /// An unconditional branch to another instruction (by index). `b target`; the
    /// byte displacement is resolved at encode time from the instruction positions.
    /// Used by the `switch` dispatch to jump to a case body or the default.
    Branch { target: usize },
    /// `bclr BO, BI` — conditional return (e.g. `bnelr`).
    BranchConditionalToLinkRegister { options: u8, condition_bit: u8 },
    /// `blr` — return to link register.
    BranchToLinkRegister,
    /// `bl target` — branch and link (call). The 24-bit displacement is filled by
    /// an `R_PPC_REL24` relocation to `target`, so the `.text` word is the
    /// placeholder `0x48000001`.
    BranchAndLink { target: String },
    /// `mflr rD` — move from the link register.
    MoveFromLinkRegister { d: u8 },
    /// `mtlr rS` — move to the link register.
    MoveToLinkRegister { s: u8 },
    /// `mtctr rS` — move to the count register (the jump-table dispatch target).
    MoveToCountRegister { s: u8 },
    /// `bctr` — branch unconditionally to the count register (`bcctr 20,0`).
    BranchToCountRegister,
    /// `bctrl` — branch to the count register and link (`bcctrl 20,0`), an indirect call.
    BranchToCountRegisterAndLink,
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

    /// Whether this is a single-precision float arithmetic instruction (opcode 59).
    /// mwcc sets the `extab` FPU flag for a leaf-with-frame that uses one of these
    /// (e.g. an `int`->`float` conversion's `fsubs`), but NOT for a double-only or
    /// convert-to-int frame (`fsub`/`fctiwz` leave the flag clear).
    pub fn is_single_precision_arithmetic(&self) -> bool {
        use Instruction::*;
        matches!(
            self,
            FloatAddSingle { .. }
                | FloatSubtractSingle { .. }
                | FloatMultiplySingle { .. }
                | FloatDivideSingle { .. }
                | FloatMultiplyAddSingle { .. }
                | FloatMultiplySubtractSingle { .. }
                | FloatNegativeMultiplySubtractSingle { .. }
        )
    }

    /// Whether this is a floating-point operation that sets the `extab` "uses FPU"
    /// flag — FPR loads/stores, arithmetic, moves, conversions. A bare float *compare*
    /// (`fcmpo`/`fcmpu`) does NOT: a non-leaf that only compares its float-register
    /// arguments (`if (a > b) ...`) leaves the flag clear in mwcc's unwind header.
    pub fn is_floating_point(&self) -> bool {
        use Instruction::*;
        matches!(
            self,
            StoreFloatSingle { .. }
                | StoreFloatSingleIndexed { .. }
                | StoreFloatDouble { .. }
                | LoadFloatDoubleIndexed { .. }
                | StoreFloatDoubleIndexed { .. }
                | LoadFloatSingle { .. }
                | LoadFloatSingleIndexed { .. }
                | LoadFloatDouble { .. }
                | FloatAddSingle { .. }
                | FloatSubtractSingle { .. }
                | FloatMultiplySingle { .. }
                | FloatDivideSingle { .. }
                | FloatMultiplyAddSingle { .. }
                | FloatMultiplySubtractSingle { .. }
                | FloatNegativeMultiplySubtractSingle { .. }
                | FloatAddDouble { .. }
                | FloatSubtractDouble { .. }
                | FloatMultiplyDouble { .. }
                | FloatMultiplyAddDouble { .. }
                | FloatMultiplySubtractDouble { .. }
                | FloatNegativeMultiplySubtractDouble { .. }
                | RoundToSingle { .. }
                | FloatMove { .. }
                | FloatNegate { .. }
                | ConvertToIntegerWordZero { .. }
        )
    }

    /// Whether this is a *single-precision* FP operation that sets the extab "uses
    /// FPU" flag. mwcc keys that flag on single precision specifically: a non-leaf
    /// (or leaf-with-frame) doing only double-precision work — `lfd`/`stfd`, a
    /// double `fadd`, a `fctiwz` convert-to-int, or a bare `fcmpo` — leaves the flag
    /// clear, so `if (d > 0.0)` against a double constant carries no FPU flag while
    /// the single-precision `if (f > 0.0f)` (an `lfs`) does. A bare single *store*
    /// does NOT count either: a non-leaf that only stores a call's float result
    /// (`gf = hf();` -> `stfs f1`) with no single load or arithmetic leaves the flag
    /// clear, matching the double-store case.
    pub fn is_single_precision_floating_point(&self) -> bool {
        use Instruction::*;
        self.is_single_precision_arithmetic()
            || matches!(
                self,
                LoadFloatSingle { .. }
                    | LoadFloatSingleIndexed { .. }
                    | RoundToSingle { .. }
            )
    }
}
