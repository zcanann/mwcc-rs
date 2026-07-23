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
    /// `addic rD, rA, SIMM` — add immediate carrying (no CR0 record). Also spells
    /// the simplified `subic rD, rA, val` mnemonic as `addic rD, rA, -val`; used by
    /// the runtime's inline-`asm` shift helpers.
    AddImmediateCarrying { d: u8, a: u8, immediate: i16 },
    /// `addis rD, rA, SIMM` — also spells `lis rD, SIMM` when `a == 0`.
    AddImmediateShifted { d: u8, a: u8, immediate: i16 },
    /// `ori rA, rS, UIMM`
    OrImmediate { a: u8, s: u8, immediate: u16 },
    /// `oris rA, rS, UIMM` — OR the immediate into the high half.
    OrImmediateShifted { a: u8, s: u8, immediate: u16 },
    /// `add rD, rA, rB`
    Add { d: u8, a: u8, b: u8 },
    /// `add.` — add with the condition-record bit.
    AddRecord { d: u8, a: u8, b: u8 },
    /// `subf rD, rA, rB` => rD = rB - rA.
    SubtractFrom { d: u8, a: u8, b: u8 },
    /// `subf. rD, rA, rB` => rD = rB - rA, recording in CR0 (the CTR-loop
    /// register-subtract head fusing its `< 0` test).
    SubtractFromRecord { d: u8, a: u8, b: u8 },
    /// `neg rD, rA`
    Negate { d: u8, a: u8 },
    /// `neg. rD, rA` — record form (sets CR0).
    NegateRecord { d: u8, a: u8 },
    /// `andi. rA, rS, UIMM` — AND immediate, ALWAYS record (no plain andi).
    AndImmediateRecord { a: u8, s: u8, immediate: u16 },
    /// `nor rA, rS, rB` — spells `not rA, rS` when `s == b`.
    Nor { a: u8, s: u8, b: u8 },
    /// `xor. rA, rS, rB` — XOR, record form (sets CR0).
    XorRecord { a: u8, s: u8, b: u8 },
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
    /// `subfe. rD, rA, rB` — record form (sets CR0); the runtime's 64-bit divide loop
    /// fuses the borrow high-word subtract with its `< 0` test.
    SubtractFromExtendedRecord { d: u8, a: u8, b: u8 },
    /// `subfze rD, rA` => rD = -rA + carry - 1 — negate-with-borrow (the runtime's
    /// signed-magnitude divide preamble).
    SubtractFromZeroExtended { d: u8, a: u8 },
    /// `addc rD, rA, rB` => rD = rA + rB, setting the carry (the low word of a 64-bit add).
    AddCarrying { d: u8, a: u8, b: u8 },
    /// `adde rD, rA, rB` => rD = rA + rB + carry.
    AddExtended { d: u8, a: u8, b: u8 },
    /// `addze rD, rA` => rD = rA + carry. Used to round a signed power-of-two
    /// division toward zero after an arithmetic shift.
    AddToZeroExtended { d: u8, a: u8 },
    /// `mullw rD, rA, rB`
    MultiplyLow { d: u8, a: u8, b: u8 },
    /// `mullw. rD, rA, rB` — multiply low word, recording CR0.
    MultiplyLowRecord { d: u8, a: u8, b: u8 },
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
    /// `or.` — OR with the record bit: writes `a` AND sets CR0 from the result,
    /// so a `(x | y) == 0` guard needs no separate compare.
    OrRecord { a: u8, s: u8, b: u8 },
    /// `and rA, rS, rB`
    And { a: u8, s: u8, b: u8 },
    /// `and. rA, rS, rB` — and, recording the result in CR0.
    AndRecord { a: u8, s: u8, b: u8 },
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
    /// `srawi. rA, rS, shift` — record form (sets CR0).
    ShiftRightAlgebraicImmediateRecord { a: u8, s: u8, shift: u8 },
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
    /// `lfdu` — load float-double AND update the base register (op 51).
    LoadFloatDoubleWithUpdate { d: u8, a: u8, offset: i16 },
    /// `lfsu fD, offset(rA)` — load float single, update rA.
    LoadFloatSingleWithUpdate { d: u8, a: u8, offset: i16 },
    /// `stfsu fS, offset(rA)` — store float single, update rA.
    StoreFloatSingleWithUpdate { s: u8, a: u8, offset: i16 },
    /// `stfdu` — store float-double AND update the base register (op 55).
    StoreFloatDoubleWithUpdate { s: u8, a: u8, offset: i16 },
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
    /// `rlwinm. rA, rS, shift, begin, end` — the general rotate-and-mask,
    /// record form (sets CR0).
    RotateAndMaskRecord { a: u8, s: u8, shift: u8, begin: u8, end: u8 },
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
    /// `fnmadds fD, fA, fC, fB` — negative multiply-add single.
    FloatNegativeMultiplyAddSingle { d: u8, a: u8, c: u8, b: u8 },
    /// `fsel frD, frA, frC, frB` — choose C when A is nonnegative, otherwise B.
    FloatSelect { d: u8, a: u8, c: u8, b: u8 },
    /// Double-precision arithmetic (opcode 63 vs the single forms' 59).
    /// `fadd frD, frA, frB`
    FloatAddDouble { d: u8, a: u8, b: u8 },
    /// `fsub frD, frA, frB`
    FloatSubtractDouble { d: u8, a: u8, b: u8 },
    /// `fmul frD, frA, frC`
    FloatMultiplyDouble { d: u8, a: u8, c: u8 },
    /// `fdiv frD, frA, frB` — double-precision divide (vs the single `fdivs`).
    FloatDivideDouble { d: u8, a: u8, b: u8 },
    /// `fmadd frD, frA, frC, frB` => frD = frA*frC + frB.
    FloatMultiplyAddDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `fmsub frD, frA, frC, frB` => frD = frA*frC - frB.
    FloatMultiplySubtractDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `fnmsub frD, frA, frC, frB` => frD = frB - frA*frC.
    FloatNegativeMultiplySubtractDouble { d: u8, a: u8, c: u8, b: u8 },
    /// `frsp frD, frB` — round a double to single precision.
    RoundToSingle { d: u8, b: u8 },
    /// `frsqrte frD, frB` — floating reciprocal square root estimate.
    FloatReciprocalSqrtEstimate { d: u8, b: u8 },
    /// `fmr frD, frB`
    FloatMove { d: u8, b: u8 },
    /// `fneg frD, frB`
    FloatNegate { d: u8, b: u8 },
    /// `fabs frD, frB` — floating absolute value.
    FloatAbsolute { d: u8, b: u8 },
    /// `fctiwz frD, frB` — convert float to integer, round toward zero.
    ConvertToIntegerWordZero { d: u8, b: u8 },
    /// `psq_l frD, offset(rA), W, I` — Gekko paired-single quantized load
    /// (the callee-saved FPR restore's second half under -proc gekko).
    PairedSingleQuantizedLoad { d: u8, a: u8, offset: i16, w: u8, i: u8 },
    /// `psq_lx frD, rA, rB, W, I` — indexed paired-single quantized load.
    /// Wii-era epilogues materialize each save offset in r0 and restore through
    /// this form rather than the immediate `psq_l` encoding.
    PairedSingleQuantizedLoadIndexed { d: u8, a: u8, b: u8, w: u8, i: u8 },
    /// `psq_st frS, offset(rA), W, I` — Gekko paired-single quantized store.
    PairedSingleQuantizedStore { s: u8, a: u8, offset: i16, w: u8, i: u8 },
    /// `stwu rS, offset(rA)` — store word with base update (stack frame push).
    StoreWordWithUpdate { s: u8, a: u8, offset: i16 },
    /// `lwz rD, offset(rA)` — load word.
    LoadWord { d: u8, a: u8, offset: i16 },
    /// `lbzu` — load byte (zero-extended) AND update the base register with
    /// the effective address (op 35).
    LoadByteZeroWithUpdate { d: u8, a: u8, offset: i16 },
    /// `lhzu d, offset(a)` — load half-word zero-extended, update `a`.
    LoadHalfZeroWithUpdate { d: u8, a: u8, offset: i16 },
    /// `stbu` — store byte AND update the base register (op 39).
    StoreByteWithUpdate { s: u8, a: u8, offset: i16 },
    /// `lwzu` — load word AND update the base register with the effective
    /// address, folding a pre-decremented element access into one instruction.
    LoadWordWithUpdate { d: u8, a: u8, offset: i16 },
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
    /// `fcmpu crfD, frA, frB` — unordered float compare into an EXPLICIT condition
    /// field (`crf != 0`; the runtime's `__cvt_fp2unsigned` uses `fcmpu cr6, …`).
    FloatCompareUnorderedField { crf: u8, a: u8, b: u8 },
    /// `mfcr rD` — move the whole condition register into a GPR.
    MoveFromConditionRegister { d: u8 },
    /// `mffs frD` — copy the FPSCR into frD (inline-asm setjmp saves it).
    MoveFromFpscr { d: u8 },
    /// `mtcrf CRM, rS` — move rS into the CR fields selected by the 8-bit mask.
    MoveToConditionRegisterFields { mask: u8, s: u8 },
    /// `mtfsf FM, frB` — move frB into the FPSCR fields selected by the 8-bit mask.
    MoveToFpscrFields { mask: u8, b: u8 },
    /// `stmw rS, d(rA)` — store rS through r31 at consecutive words.
    StoreMultipleWord { s: u8, a: u8, offset: i16 },
    /// `lmw rD, d(rA)` — load rD through r31 from consecutive words.
    LoadMultipleWord { d: u8, a: u8, offset: i16 },
    /// `crclr crbD` — clear one condition-register bit (`crxor d, d, d`).
    ConditionRegisterClear { d: u8 },
    /// `cror crbD, crbA, crbB` — OR two condition-register bits into a third.
    /// Bit numbers are absolute (cr0 occupies bits 0..=3: lt=0, gt=1, eq=2, so=3).
    ConditionRegisterOr { d: u8, a: u8, b: u8 },
    /// `cmpwi crf0, rA, SIMM` — signed compare against an immediate.
    CompareWordImmediate { a: u8, immediate: i16 },
    /// `cmpwi crfD, rA, SIMM` — signed immediate compare into an EXPLICIT condition
    /// field (`crf != 0`; the runtime's inline-`asm` `__mod2i` uses `cmpwi cr7, …`).
    CompareWordImmediateField { crf: u8, a: u8, immediate: i16 },
    /// `cmpw crf, rA, rB` — signed register compare into a NON-cr0 field.
    CompareWordField { crf: u8, a: u8, b: u8 },
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
    /// `blrl` — branch through the link register and link (legacy indirect call).
    BranchToLinkRegisterAndLink,
    /// `bl target` — branch and link (call). The 24-bit displacement is filled by
    /// an `R_PPC_REL24` relocation to `target`, so the `.text` word is the
    /// placeholder `0x48000001`.
    BranchAndLink { target: String },
    /// `b target` — external sibling/tail call. The 24-bit displacement is filled
    /// by an `R_PPC_REL24` relocation, so the `.text` word is `0x48000000`.
    BranchExternal { target: String },
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
    /// `mfspr rD, SPR` — move from a special-purpose register (the SPR number
    /// carries the raw value; the split-field encoding is applied at encode time).
    MoveFromSpr { d: u8, spr: u16 },
    /// `mftb rD, TBR` — move from a time-base register (XO 371, distinct from `mfspr`).
    MoveFromTimeBase { d: u8, tbr: u16 },
    /// `mtspr SPR, rS` — move to a special-purpose register.
    MoveToSpr { spr: u16, s: u8 },
    /// `mfsr rD, SR` — move from one of the sixteen segment registers.
    MoveFromSegmentRegister { d: u8, segment: u8 },
    /// `mtsr SR, rS` — move to one of the sixteen segment registers.
    MoveToSegmentRegister { segment: u8, s: u8 },
    /// `mfmsr rD` — move from the machine-state register.
    MoveFromMsr { d: u8 },
    /// `mtmsr rS` — move to the machine-state register.
    MoveToMsr { s: u8 },
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
    CacheOp { primary: u8, xo: u16, a: u8, b: u8 },
    /// `sc` — system call.
    SystemCall,
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
                | PairedSingleQuantizedLoad { .. }
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
}
