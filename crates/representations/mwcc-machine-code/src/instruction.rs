//! The `Instruction` representation: structured PowerPC (Gekko) instructions the
//! allocator and scheduler can inspect and rewrite before encoding.

use crate::RegisterField;

/// A selected PowerPC instruction. Register fields may hold allocation identities
/// during selection; encoding requires physical register numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    /// `addi rD, rA, SIMM` — also spells `li rD, SIMM` when `a == 0`.
    AddImmediate { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `addic. rD, rA, SIMM` — add immediate carrying, recording the result in CR0
    /// (so a following `bne`/`beq` tests it). Used by a loop counter decrement
    /// (`--n` → `addic. rN, rN, -1`).
    AddImmediateCarryingRecord { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `addic rD, rA, SIMM` — add immediate carrying (no CR0 record). Also spells
    /// the simplified `subic rD, rA, val` mnemonic as `addic rD, rA, -val`; used by
    /// the runtime's inline-`asm` shift helpers.
    AddImmediateCarrying { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `addis rD, rA, SIMM` — also spells `lis rD, SIMM` when `a == 0`.
    AddImmediateShifted { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `ori rA, rS, UIMM`
    OrImmediate { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `oris rA, rS, UIMM` — OR the immediate into the high half.
    OrImmediateShifted { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `add rD, rA, rB`
    Add { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `add.` — add with the condition-record bit.
    AddRecord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `subf rD, rA, rB` => rD = rB - rA.
    SubtractFrom { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `subf. rD, rA, rB` => rD = rB - rA, recording in CR0 (the CTR-loop
    /// register-subtract head fusing its `< 0` test).
    SubtractFromRecord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `neg rD, rA`
    Negate { d: RegisterField, a: RegisterField },
    /// `neg. rD, rA` — record form (sets CR0).
    NegateRecord { d: RegisterField, a: RegisterField },
    /// `andi. rA, rS, UIMM` — AND immediate, ALWAYS record (no plain andi).
    AndImmediateRecord { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `andis.` — mask the high half and record the result in CR0.
    AndImmediateShiftedRecord { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `nor rA, rS, rB` — spells `not rA, rS` when `s == b`.
    Nor { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `xor. rA, rS, rB` — XOR, record form (sets CR0).
    XorRecord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `nand rA, rS, rB` — `~(rS & rB)`.
    Nand { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `eqv rA, rS, rB` — `~(rS ^ rB)`.
    Eqv { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `cntlzw rA, rS` — count leading zero bits.
    CountLeadingZeros { a: RegisterField, s: RegisterField },
    /// `extsb rA, rS` — sign-extend byte.
    ExtendSignByte { a: RegisterField, s: RegisterField },
    /// `extsb. rA, rS` — sign-extend byte, record form (sets cr0). mwcc uses this to
    /// test a signed `char` condition: sign-extend and compare against 0 in one.
    ExtendSignByteRecord { a: RegisterField, s: RegisterField },
    /// `extsh rA, rS` — sign-extend halfword.
    ExtendSignHalfword { a: RegisterField, s: RegisterField },
    /// `extsh. rA, rS` — sign-extend halfword, record form (sets cr0). mwcc uses this
    /// to test a signed `short` against 0 in one instruction.
    ExtendSignHalfwordRecord { a: RegisterField, s: RegisterField },
    /// `andc rA, rS, rB` => rA = rS & ~rB.
    AndComplement { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `orc rA, rS, rB` => rA = rS | ~rB.
    OrComplement { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `subfic rD, rA, SIMM` => rD = SIMM - rA.
    SubtractFromImmediate { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `subfc rD, rA, rB` => rD = rB - rA, setting the carry.
    SubtractFromCarrying { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `subfe rD, rA, rB` => rD = rB - rA + carry - 1 (the carrying high word of a 64-bit subtract).
    SubtractFromExtended { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `subfe. rD, rA, rB` — record form (sets CR0); the runtime's 64-bit divide loop
    /// fuses the borrow high-word subtract with its `< 0` test.
    SubtractFromExtendedRecord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `subfze rD, rA` => rD = -rA + carry - 1 — negate-with-borrow (the runtime's
    /// signed-magnitude divide preamble).
    SubtractFromZeroExtended { d: RegisterField, a: RegisterField },
    /// `addc rD, rA, rB` => rD = rA + rB, setting the carry (the low word of a 64-bit add).
    AddCarrying { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `adde rD, rA, rB` => rD = rA + rB + carry.
    AddExtended { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `addze rD, rA` => rD = rA + carry. Used to round a signed power-of-two
    /// division toward zero after an arithmetic shift.
    AddToZeroExtended { d: RegisterField, a: RegisterField },
    /// `mullw rD, rA, rB`
    MultiplyLow { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `mullw. rD, rA, rB` — multiply low word, recording CR0.
    MultiplyLowRecord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `mulhw rD, rA, rB` — high 32 bits of the signed product.
    MultiplyHighWord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `mulhwu rD, rA, rB` — high 32 bits of the unsigned product.
    MultiplyHighWordUnsigned { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `mulli rD, rA, SIMM`
    MultiplyImmediate { d: RegisterField, a: RegisterField, immediate: i16 },
    /// `divw rD, rA, rB` — signed divide.
    DivideWord { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `divwu rD, rA, rB` — unsigned divide.
    DivideWordUnsigned { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `slwi rA, rS, shift` — shift left by `shift` (1..=31), via `rlwinm`.
    ShiftLeftImmediate { a: RegisterField, s: RegisterField, shift: u8 },
    /// `or rA, rS, rB` — spells `mr rA, rS` when `s == b`.
    Or { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `or.` — OR with the record bit: writes `a` AND sets CR0 from the result,
    /// so a `(x | y) == 0` guard needs no separate compare.
    OrRecord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `and rA, rS, rB`
    And { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `and. rA, rS, rB` — and, recording the result in CR0.
    AndRecord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `xor rA, rS, rB`
    Xor { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `slw rA, rS, rB` — shift left word by the low bits of rB.
    ShiftLeftWord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `sraw rA, rS, rB` — arithmetic (signed) shift right word.
    ShiftRightAlgebraicWord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `srw rA, rS, rB` — logical (unsigned) shift right word.
    ShiftRightWord { a: RegisterField, s: RegisterField, b: RegisterField },
    /// `srawi rA, rS, shift` — arithmetic shift right immediate.
    ShiftRightAlgebraicImmediate { a: RegisterField, s: RegisterField, shift: u8 },
    /// `srawi. rA, rS, shift` — record form (sets CR0).
    ShiftRightAlgebraicImmediateRecord { a: RegisterField, s: RegisterField, shift: u8 },
    /// `srwi rA, rS, shift` — logical shift right immediate, via `rlwinm`.
    ShiftRightLogicalImmediate { a: RegisterField, s: RegisterField, shift: u8 },
    /// `xori rA, rS, UIMM`
    XorImmediate { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `xoris rA, rS, UIMM`
    XorImmediateShifted { a: RegisterField, s: RegisterField, immediate: u16 },
    /// `stw rS, offset(rA)` — store word.
    StoreWord { s: RegisterField, a: RegisterField, offset: i16 },
    /// `stb rS, offset(rA)` — store byte.
    StoreByte { s: RegisterField, a: RegisterField, offset: i16 },
    /// `sth rS, offset(rA)` — store halfword.
    StoreHalfword { s: RegisterField, a: RegisterField, offset: i16 },
    /// `stfs frS, offset(rA)` — store float single.
    StoreFloatSingle { s: RegisterField, a: RegisterField, offset: i16 },
    /// `stwx rS, rA, rB` — store word indexed.
    StoreWordIndexed { s: RegisterField, a: RegisterField, b: RegisterField },
    /// `stbx rS, rA, rB` — store byte indexed.
    StoreByteIndexed { s: RegisterField, a: RegisterField, b: RegisterField },
    /// `sthx rS, rA, rB` — store halfword indexed.
    StoreHalfwordIndexed { s: RegisterField, a: RegisterField, b: RegisterField },
    /// `stfsx frS, rA, rB` — store float single indexed.
    StoreFloatSingleIndexed { s: RegisterField, a: RegisterField, b: RegisterField },
    /// `lfd frD, offset(rA)` — load float double.
    LoadFloatDouble { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lfdx frD, rA, rB` — load float double indexed.
    LoadFloatDoubleIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `stfdx frS, rA, rB` — store float double indexed.
    StoreFloatDoubleIndexed { s: RegisterField, a: RegisterField, b: RegisterField },
    /// `lfdu` — load float-double AND update the base register (op 51).
    LoadFloatDoubleWithUpdate { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lfsu fD, offset(rA)` — load float single, update rA.
    LoadFloatSingleWithUpdate { d: RegisterField, a: RegisterField, offset: i16 },
    /// `stfsu fS, offset(rA)` — store float single, update rA.
    StoreFloatSingleWithUpdate { s: RegisterField, a: RegisterField, offset: i16 },
    /// `stfdu` — store float-double AND update the base register (op 55).
    StoreFloatDoubleWithUpdate { s: RegisterField, a: RegisterField, offset: i16 },
    /// `clrlwi rA, rS, n` — clear the high `n` bits (mask to the low `32-n`), via `rlwinm`.
    ClearLeftImmediate { a: RegisterField, s: RegisterField, clear: u8 },
    /// `clrlwi. rA, rS, n` — record form (`rlwinm.`): zero-extend a narrow unsigned
    /// value and set cr0, mwcc's one-instruction test of an unsigned `char`/`short`
    /// against 0.
    ClearLeftImmediateRecord { a: RegisterField, s: RegisterField, clear: u8 },
    /// `rlwinm rA, rS, 0, begin, end` — keep the contiguous bit run `[begin, end]`.
    AndContiguousMask { a: RegisterField, s: RegisterField, begin: u8, end: u8 },
    /// `rlwinm rA, rS, shift, begin, end` — rotate left by `shift`, keep bits
    /// `[begin, end]`. The general form; mwcc fuses a narrow unsigned shift and
    /// its width mask into one of these.
    RotateAndMask { a: RegisterField, s: RegisterField, shift: u8, begin: u8, end: u8 },
    /// `rlwinm. rA, rS, shift, begin, end` — the general rotate-and-mask,
    /// record form (sets CR0).
    RotateAndMaskRecord { a: RegisterField, s: RegisterField, shift: u8, begin: u8, end: u8 },
    /// `rlwnm rA, rS, rB, begin, end` — like `rlwinm` but the rotate amount is the
    /// low five bits of `rB` (a register) rather than an immediate. mwcc uses it for
    /// the `x <= 0` idiom: rotating a `1` left by `cntlzw(x)` lands in the low bit
    /// only when the leading-zero count is 0 or 32 (i.e. `x < 0` or `x == 0`).
    RotateAndMaskVariable { a: RegisterField, s: RegisterField, b: RegisterField, begin: u8, end: u8 },
    /// `rlwimi rA, rS, shift, begin, end` — rotate `rS` left by `shift` and insert
    /// bits `[begin, end]` into `rA`, leaving `rA`'s other bits intact. mwcc uses
    /// it to merge two disjoint bit fields (e.g. an OR of two shifts, or a masked
    /// sign/magnitude merge) into one instruction.
    RotateAndMaskInsert { a: RegisterField, s: RegisterField, shift: u8, begin: u8, end: u8 },
    /// `rlwinm. rA, rS, 0, begin, end` — keep the bit run `[begin, end]` of `rS`
    /// and set cr0 from the result. Used to test `(x & mask)` in a condition.
    AndMaskRecord { a: RegisterField, s: RegisterField, begin: u8, end: u8 },
    /// `fadds frD, frA, frB`
    FloatAddSingle { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fsubs frD, frA, frB`
    FloatSubtractSingle { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fmuls frD, frA, frC`
    FloatMultiplySingle { d: RegisterField, a: RegisterField, c: RegisterField },
    /// `fdivs frD, frA, frB`
    FloatDivideSingle { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fmadds frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddSingle { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fmsubs frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractSingle { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fnmsubs frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractSingle { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fnmadds fD, fA, fC, fB` — negative multiply-add single.
    FloatNegativeMultiplyAddSingle { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fsel frD, frA, frC, frB` — choose C when A is nonnegative, otherwise B.
    FloatSelect { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// Double-precision arithmetic (opcode 63 vs the single forms' 59).
    /// `fadd frD, frA, frB`
    FloatAddDouble { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fsub frD, frA, frB`
    FloatSubtractDouble { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fmul frD, frA, frC`
    FloatMultiplyDouble { d: RegisterField, a: RegisterField, c: RegisterField },
    /// `fdiv frD, frA, frB` — double-precision divide (vs the single `fdivs`).
    FloatDivideDouble { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `fmadd frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddDouble { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fmsub frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractDouble { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `fnmsub frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractDouble { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `frsp frD, frB` — round a double to single precision.
    RoundToSingle { d: RegisterField, b: RegisterField },
    /// `frsqrte frD, frB` — floating reciprocal square root estimate.
    FloatReciprocalSqrtEstimate { d: RegisterField, b: RegisterField },
    /// `fmr frD, frB`
    FloatMove { d: RegisterField, b: RegisterField },
    /// `ps_add frD, frA, frB` — add both paired-single lanes.
    PairedSingleAdd { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `ps_sub frD, frA, frB` — subtract both paired-single lanes.
    PairedSingleSubtract { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `ps_mul frD, frA, frC` — multiply both paired-single lanes.
    PairedSingleMultiply { d: RegisterField, a: RegisterField, c: RegisterField },
    /// `ps_muls0 frD, frA, frC` — multiply both lanes by lane zero of `frC`.
    PairedSingleMultiplyScalar0 { d: RegisterField, a: RegisterField, c: RegisterField },
    /// `ps_madd frD, frA, frC, frB` — multiply-add both paired-single lanes.
    PairedSingleMultiplyAdd { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `ps_sum0 frD, frA, frC, frB` — sum lane zero and copy lane one from `frB`.
    PairedSingleSum0 { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `ps_sum1 frD, frA, frC, frB` — copy lane zero from `frB` and sum lane one.
    PairedSingleSum1 { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    /// `ps_mr frD, frB` — Gekko paired-single register move.
    PairedSingleMove { d: RegisterField, b: RegisterField },
    /// Merge selected lanes of two paired-single registers (`ps_merge00` etc.).
    PairedSingleMerge00 { d: RegisterField, a: RegisterField, b: RegisterField },
    PairedSingleMerge01 { d: RegisterField, a: RegisterField, b: RegisterField },
    PairedSingleMerge10 { d: RegisterField, a: RegisterField, b: RegisterField },
    PairedSingleMerge11 { d: RegisterField, a: RegisterField, b: RegisterField },
    /// Multiply both lanes by lane one of `c` (`ps_muls1`).
    PairedSingleMultiplyScalar1 { d: RegisterField, a: RegisterField, c: RegisterField },
    /// Multiply by the selected lane of `c`, then add both lanes of `b`.
    PairedSingleMultiplyAddScalar0 { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },
    PairedSingleMultiplyAddScalar1 { d: RegisterField, a: RegisterField, c: RegisterField, b: RegisterField },

    /// `fneg frD, frB`
    FloatNegate { d: RegisterField, b: RegisterField },
    /// `fabs frD, frB` — floating absolute value.
    FloatAbsolute { d: RegisterField, b: RegisterField },
    /// `fctiwz frD, frB` — convert float to integer, round toward zero.
    ConvertToIntegerWordZero { d: RegisterField, b: RegisterField },
    /// `psq_l frD, offset(rA), W, I` — Gekko paired-single quantized load
    /// (the callee-saved FPR restore's second half under -proc gekko).
    PairedSingleQuantizedLoad { d: RegisterField, a: RegisterField, offset: i16, w: u8, i: u8 },
    /// `psq_lx frD, rA, rB, W, I` — indexed paired-single quantized load.
    /// Large-frame epilogues materialize an offset outside `psq_l`'s signed
    /// 12-bit displacement in r0 and restore through this form.
    PairedSingleQuantizedLoadIndexed { d: RegisterField, a: RegisterField, b: RegisterField, w: u8, i: u8 },
    /// `psq_st frS, offset(rA), W, I` — Gekko paired-single quantized store.
    PairedSingleQuantizedStore { s: RegisterField, a: RegisterField, offset: i16, w: u8, i: u8 },
    /// Quantized load/store with effective-address writeback to the base GPR.
    PairedSingleQuantizedLoadWithUpdate { d: RegisterField, a: RegisterField, offset: i16, w: u8, i: u8 },
    PairedSingleQuantizedStoreWithUpdate { s: RegisterField, a: RegisterField, offset: i16, w: u8, i: u8 },
    /// `stwu rS, offset(rA)` — store word with base update (stack frame push).
    StoreWordWithUpdate { s: RegisterField, a: RegisterField, offset: i16 },
    /// `lwz rD, offset(rA)` — load word.
    LoadWord { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lbzu` — load byte (zero-extended) AND update the base register with
    /// the effective address (op 35).
    LoadByteZeroWithUpdate { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lhzu d, offset(a)` — load half-word zero-extended, update `a`.
    LoadHalfZeroWithUpdate { d: RegisterField, a: RegisterField, offset: i16 },
    /// `stbu` — store byte AND update the base register (op 39).
    StoreByteWithUpdate { s: RegisterField, a: RegisterField, offset: i16 },
    /// `lwzu` — load word AND update the base register with the effective
    /// address, folding a pre-decremented element access into one instruction.
    LoadWordWithUpdate { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lbz rD, offset(rA)` — load byte, zero-extended.
    LoadByteZero { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lhz rD, offset(rA)` — load halfword, zero-extended.
    LoadHalfwordZero { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lha rD, offset(rA)` — load halfword, sign-extended.
    LoadHalfwordAlgebraic { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lfs frD, offset(rA)` — load float single.
    LoadFloatSingle { d: RegisterField, a: RegisterField, offset: i16 },
    /// `lwzx rD, rA, rB` — load word indexed.
    LoadWordIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `lbzx rD, rA, rB` — load byte indexed, zero-extended.
    LoadByteZeroIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `lhzx rD, rA, rB` — load halfword indexed, zero-extended.
    LoadHalfwordZeroIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `lhax rD, rA, rB` — load halfword indexed, sign-extended.
    LoadHalfwordAlgebraicIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `lfsx frD, rA, rB` — load float single indexed.
    LoadFloatSingleIndexed { d: RegisterField, a: RegisterField, b: RegisterField },
    /// `stfd frS, offset(rA)` — store float double.
    StoreFloatDouble { s: RegisterField, a: RegisterField, offset: i16 },
    /// `fcmpo crf0, frA, frB` — ordered float compare.
    FloatCompareOrdered { a: RegisterField, b: RegisterField },
    /// `fcmpu crf0, frA, frB` — unordered float compare (mwcc uses this for `==`/`!=`).
    FloatCompareUnordered { a: RegisterField, b: RegisterField },
    /// `fcmpu crfD, frA, frB` — unordered float compare into an EXPLICIT condition
    /// field (`crf != 0`; the runtime's `__cvt_fp2unsigned` uses `fcmpu cr6, …`).
    FloatCompareUnorderedField { crf: u8, a: RegisterField, b: RegisterField },
    /// `mfcr rD` — move the whole condition register into a GPR.
    MoveFromConditionRegister { d: RegisterField },
    /// `mffs frD` — copy the FPSCR into frD (inline-asm setjmp saves it).
    MoveFromFpscr { d: RegisterField },
    /// `mtcrf CRM, rS` — move rS into the CR fields selected by the 8-bit mask.
    MoveToConditionRegisterFields { mask: u8, s: RegisterField },
    /// `mtfsf FM, frB` — move frB into the FPSCR fields selected by the 8-bit mask.
    MoveToFpscrFields { mask: u8, b: RegisterField },
    /// `stmw rS, d(rA)` — store rS through r31 at consecutive words.
    StoreMultipleWord { s: RegisterField, a: RegisterField, offset: i16 },
    /// `lmw rD, d(rA)` — load rD through r31 from consecutive words.
    LoadMultipleWord { d: RegisterField, a: RegisterField, offset: i16 },
    /// `crclr crbD` — clear one condition-register bit (`crxor d, d, d`).
    ConditionRegisterClear { d: u8 },
    /// `crset crbD` — set one condition-register bit (`creqv d, d, d`).
    ConditionRegisterSet { d: u8 },
    /// `cror crbD, crbA, crbB` — OR two condition-register bits into a third.
    /// Bit numbers are absolute (cr0 occupies bits 0..=3: lt=0, gt=1, eq=2, so=3).
    ConditionRegisterOr { d: u8, a: u8, b: u8 },
    /// `cmpwi crf0, rA, SIMM` — signed compare against an immediate.
    CompareWordImmediate { a: RegisterField, immediate: i16 },
    /// `cmpwi crfD, rA, SIMM` — signed immediate compare into an EXPLICIT condition
    /// field (`crf != 0`; the runtime's inline-`asm` `__mod2i` uses `cmpwi cr7, …`).
    CompareWordImmediateField { crf: u8, a: RegisterField, immediate: i16 },
    /// `cmpw crf, rA, rB` — signed register compare into a NON-cr0 field.
    CompareWordField { crf: u8, a: RegisterField, b: RegisterField },
    /// `cmpw crf0, rA, rB` — signed compare.
    CompareWord { a: RegisterField, b: RegisterField },
    /// `cmplwi crf0, rA, UIMM` — unsigned compare against an immediate.
    CompareLogicalWordImmediate { a: RegisterField, immediate: u16 },
    /// `cmplw crf0, rA, rB` — unsigned compare.
    CompareLogicalWord { a: RegisterField, b: RegisterField },
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
    /// `blrl` — branch through the link register and link (legacy indirect call).
    BranchToLinkRegisterAndLink,
    /// `bl target` — branch and link (call). The 24-bit displacement is filled by
    /// an `R_PPC_REL24` relocation to `target`, so the `.text` word is the
    /// placeholder `0x48000001`.
    BranchAndLink { target: String },
    /// I-form branch with a byte-valued LI field, optional AA and LK bits.
    /// Unlike `Branch`, `value` is not an instruction index and is never
    /// retargeted when instructions move. A symbol relocation may patch it.
    BranchImmediate { value: i32, absolute: bool, link: bool },
    /// `b target` — external sibling/tail call. The 24-bit displacement is filled
    /// by an `R_PPC_REL24` relocation, so the `.text` word is `0x48000000`.
    BranchExternal { target: String },
    /// `mflr rD` — move from the link register.
    MoveFromLinkRegister { d: RegisterField },
    /// `mtlr rS` — move to the link register.
    MoveToLinkRegister { s: RegisterField },
    /// `mtctr rS` — move to the count register (the jump-table dispatch target).
    MoveToCountRegister { s: RegisterField },
    /// `bctr` — branch unconditionally to the count register (`bcctr 20,0`).
    BranchToCountRegister,
    /// `bctrl` — branch to the count register and link (`bcctrl 20,0`), an indirect call.
    BranchToCountRegisterAndLink,
    /// `mfspr rD, SPR` — move from a special-purpose register (the SPR number
    /// carries the raw value; the split-field encoding is applied at encode time).
    MoveFromSpr { d: RegisterField, spr: u16 },
    /// `mftb rD, TBR` — move from a time-base register (XO 371, distinct from `mfspr`).
    MoveFromTimeBase { d: RegisterField, tbr: u16 },
    /// `mtspr SPR, rS` — move to a special-purpose register.
    MoveToSpr { spr: u16, s: RegisterField },
    /// `mfsr rD, SR` — move from one of the sixteen segment registers.
    MoveFromSegmentRegister { d: RegisterField, segment: u8 },
    /// `mtsr SR, rS` — move to one of the sixteen segment registers.
    MoveToSegmentRegister { segment: u8, s: RegisterField },
    /// `mfmsr rD` — move from the machine-state register.
    MoveFromMsr { d: RegisterField },
    /// `mtmsr rS` — move to the machine-state register.
    MoveToMsr { s: RegisterField },
    /// `isync` — instruction synchronize.
    InstructionSynchronize,
    /// `sync` (a.k.a. `hwsync`) — storage synchronize.
    Synchronize,
    /// `eieio` — enforce in-order execution of I/O.
    EnforceInOrderIo,
    /// `rfi` — return from interrupt.
    ReturnFromInterrupt,
    /// A cache-block op (`dcbf`/`dcbi`/`dcbst`/`dcbt`/`dcbz`/`dcbz_l`/`icbi`) —
    /// `op rA, rB`, addressing `(rA|0) + rB`. Carries its primary opcode (31, or
    /// 4 for the Gekko `dcbz_l`) and extended opcode. Inline-asm only.
    CacheOp { primary: u8, xo: u16, a: RegisterField, b: RegisterField },
    /// `sc` — system call.
    SystemCall,
    /// One already-scheduled PowerPC word owned by an exact whole-function
    /// capture. This deliberately has no register description: capture owners
    /// must mark their output `pre_scheduled` and attach relocations separately.
    /// General instruction selection and inline assembly must use structured
    /// variants so allocation and scheduling can still inspect their operands.
    VerbatimWord(u32),
}

impl Instruction {
    /// Calls define the ABI volatile registers and preserve fallthrough flow.
    pub fn is_call(&self) -> bool {
        matches!(self, Self::BranchAndLink { .. }
            | Self::BranchImmediate { link: true, .. }
            | Self::BranchToLinkRegisterAndLink
            | Self::BranchToCountRegisterAndLink)
    }

    /// `li rD, SIMM`
    pub fn load_immediate(d: RegisterField, immediate: i16) -> Self {
        Instruction::AddImmediate { d, a: 0, immediate }
    }
    /// `lis rD, SIMM`
    pub fn load_immediate_shifted(d: RegisterField, immediate: i16) -> Self {
        Instruction::AddImmediateShifted { d, a: 0, immediate }
    }
    /// `mr rA, rS`
    pub fn move_register(a: RegisterField, s: RegisterField) -> Self {
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
                | PairedSingleAdd { .. }
                | PairedSingleSubtract { .. }
                | PairedSingleMultiply { .. }
                | PairedSingleMultiplyScalar0 { .. }
                | PairedSingleMultiplyScalar1 { .. }
                | PairedSingleMultiplyAddScalar0 { .. }
                | PairedSingleMultiplyAddScalar1 { .. }
                | PairedSingleMultiplyAdd { .. }
                | PairedSingleSum0 { .. }
                | PairedSingleSum1 { .. }
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
                | PairedSingleQuantizedLoad { .. }
                | PairedSingleQuantizedLoadWithUpdate { .. }
                | PairedSingleQuantizedStoreWithUpdate { .. }
                | PairedSingleQuantizedLoadIndexed { .. }
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
                | FloatDivideDouble { .. }
                | FloatMultiplyAddDouble { .. }
                | FloatMultiplySubtractDouble { .. }
                | FloatNegativeMultiplySubtractDouble { .. }
                | FloatSelect { .. }
                | RoundToSingle { .. }
                | FloatMove { .. }
                | PairedSingleAdd { .. }
                | PairedSingleSubtract { .. }
                | PairedSingleMultiply { .. }
                | PairedSingleMultiplyScalar0 { .. }
                | PairedSingleMultiplyScalar1 { .. }
                | PairedSingleMultiplyAddScalar0 { .. }
                | PairedSingleMultiplyAddScalar1 { .. }
                | PairedSingleMultiplyAdd { .. }
                | PairedSingleSum0 { .. }
                | PairedSingleSum1 { .. }
                | PairedSingleMove { .. }
                | PairedSingleMerge00 { .. }
                | PairedSingleMerge01 { .. }
                | PairedSingleMerge10 { .. }
                | PairedSingleMerge11 { .. }
                | FloatNegate { .. }
                | FloatAbsolute { .. }
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

    /// The FPR defined by this instruction, when it has one. Comparisons and
    /// stores only consume FPRs and therefore return `None`.
    pub fn float_destination(&self) -> Option<RegisterField> {
        use Instruction::*;
        match self {
            LoadFloatDouble { d, .. }
            | LoadFloatDoubleIndexed { d, .. }
            | LoadFloatDoubleWithUpdate { d, .. }
            | LoadFloatSingle { d, .. }
            | LoadFloatSingleIndexed { d, .. }
            | LoadFloatSingleWithUpdate { d, .. }
            | PairedSingleQuantizedLoad { d, .. }
            | PairedSingleQuantizedLoadWithUpdate { d, .. }
            | PairedSingleQuantizedLoadIndexed { d, .. }
            | FloatAddSingle { d, .. }
            | FloatSubtractSingle { d, .. }
            | FloatMultiplySingle { d, .. }
            | FloatDivideSingle { d, .. }
            | FloatMultiplyAddSingle { d, .. }
            | FloatMultiplySubtractSingle { d, .. }
            | FloatNegativeMultiplySubtractSingle { d, .. }
            | FloatNegativeMultiplyAddSingle { d, .. }
            | PairedSingleAdd { d, .. }
            | PairedSingleSubtract { d, .. }
            | PairedSingleMultiply { d, .. }
            | PairedSingleMultiplyScalar0 { d, .. }
            | PairedSingleMultiplyScalar1 { d, .. }
            | PairedSingleMultiplyAddScalar0 { d, .. }
            | PairedSingleMultiplyAddScalar1 { d, .. }
            | PairedSingleMultiplyAdd { d, .. }
            | PairedSingleSum0 { d, .. }
            | PairedSingleSum1 { d, .. }
            | FloatAddDouble { d, .. }
            | FloatSubtractDouble { d, .. }
            | FloatMultiplyDouble { d, .. }
            | FloatDivideDouble { d, .. }
            | FloatMultiplyAddDouble { d, .. }
            | FloatMultiplySubtractDouble { d, .. }
            | FloatNegativeMultiplySubtractDouble { d, .. }
            | FloatSelect { d, .. }
            | RoundToSingle { d, .. }
            | FloatReciprocalSqrtEstimate { d, .. }
            | FloatMove { d, .. }
            | PairedSingleMove { d, .. }
            | PairedSingleMerge00 { d, .. }
            | PairedSingleMerge01 { d, .. }
            | PairedSingleMerge10 { d, .. }
            | PairedSingleMerge11 { d, .. }
            | FloatNegate { d, .. }
            | FloatAbsolute { d, .. }
            | ConvertToIntegerWordZero { d, .. }
            | MoveFromFpscr { d } => Some(*d),
            _ => None,
        }
    }
}
