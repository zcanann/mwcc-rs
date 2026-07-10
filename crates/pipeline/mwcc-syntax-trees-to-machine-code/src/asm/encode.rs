//! Per-line assembly: turn one parsed `asm` line into an [`Instruction`].
//!
//! [`assemble_line`] is the big mnemonic match; the operand-extraction helpers it
//! leans on live in [`super::operands`]. Branch mnemonics resolve their target
//! label through the driver's label map and honor the `+`/`-` prediction hints.

use super::operands::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{AsmInstruction, AsmOperand};
use std::collections::HashMap;

/// Assemble one asm line into an instruction, or `None` for a directive that
/// emits nothing (`nofralloc`). Branch mnemonics resolve their target label
/// through `labels` (label name -> instruction index).
pub(super) fn assemble_line(line: &AsmInstruction, labels: &HashMap<&str, usize>, instruction_index: usize) -> Compilation<Option<Instruction>> {
    let raw = line.mnemonic.as_str();
    // A branch static-prediction hint: `+` predicts taken, `-` predicts not taken.
    // The BO "y" bit is set only when the requested prediction DIFFERS from the
    // default implied by the branch direction (backward = taken, forward = not
    // taken). `assemble_branch` resolves that once the target is known.
    let (mnemonic, hint) = match raw.strip_suffix('+') {
        Some(base) => (base, BranchHint::Taken),
        None => match raw.strip_suffix('-') {
            Some(base) => (base, BranchHint::NotTaken),
            None => (raw, BranchHint::None),
        },
    };
    let operands = &line.operands;
    let instruction = match mnemonic {
        // `nofralloc` suppresses the auto-generated stack frame. For the register-
        // only bodies supported so far no frame is generated regardless, so it is a
        // no-op directive; it emits nothing.
        "nofralloc" => return Ok(None),
        // `frfree` releases the FP registers for the allocator — a directive with no
        // frame in these bodies, so it emits nothing (like `nofralloc`).
        "frfree" => return Ok(None),
        // `mr rA, rS` — register move (`or rA, rS, rS`).
        "mr" => {
            let [a, s] = gprs(mnemonic, operands)?;
            Instruction::move_register(a, s)
        }
        // `li rD, SIMM` — load immediate (`addi rD, r0, SIMM`).
        "li" => {
            let (d, immediate) = gpr_immediate(mnemonic, operands)?;
            Instruction::load_immediate(d, immediate)
        }
        // `lis rD, SIMM` — load immediate shifted (`addis rD, r0, SIMM`). The
        // immediate may be a `sym@h`/`@ha` relocation (assembled as 0, patched later).
        "lis" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let d = gpr(mnemonic, &operands[0])?;
            let immediate = signed_immediate_or_symbol(mnemonic, &operands[1])?;
            Instruction::load_immediate_shifted(d, immediate)
        }
        // `blr` — return (branch to link register).
        "blr" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchToLinkRegister
        }

        // Three-register ALU (`op dst, srcA, srcB`), each mapped positionally.
        "add" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::Add { d, a, b } }
        "subf" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::SubtractFrom { d, a, b } }
        // `sub rD, rA, rB` (rD = rA - rB) is the simplified spelling of `subf rD, rB, rA`.
        "sub" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::SubtractFrom { d, a: b, b: a } }
        "subfc" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::SubtractFromCarrying { d, a, b } }
        "subfe" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::SubtractFromExtended { d, a, b } }
        "adde" => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::AddExtended { d, a, b } }
        "or" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::Or { a, s, b } }
        "and" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::And { a, s, b } }
        "xor" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::Xor { a, s, b } }
        "nor" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::Nor { a, s, b } }
        "slw" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::ShiftLeftWord { a, s, b } }
        "srw" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::ShiftRightWord { a, s, b } }

        // Two-register ALU (`op dst, src`).
        "neg" => { let [d, a] = gprs(mnemonic, operands)?; Instruction::Negate { d, a } }
        "cntlzw" => { let [a, s] = gprs(mnemonic, operands)?; Instruction::CountLeadingZeros { a, s } }
        "sraw" => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::ShiftRightAlgebraicWord { a, s, b } }
        "srawi" => { let (a, s, shift) = rr_shift(mnemonic, operands)?; Instruction::ShiftRightAlgebraicImmediate { a, s, shift } }
        "addze" => { let [d, a] = gprs(mnemonic, operands)?; Instruction::AddToZeroExtended { d, a } }

        // Add-immediate-carrying (`addic dst, src, SIMM`); the simplified `subic`
        // spelling negates the immediate (`subic d, a, v` == `addic d, a, -v`).
        "addic" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediateCarrying { d, a, immediate } }
        "subic" => {
            let (d, a, immediate) = rri(mnemonic, operands)?;
            let immediate = immediate.checked_neg().ok_or_else(|| Diagnostic::error("inline-asm 'subic' immediate overflows on negation"))?;
            Instruction::AddImmediateCarrying { d, a, immediate }
        }
        "addic." => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediateCarryingRecord { d, a, immediate } }
        "subfze" => { let [d, a] = gprs(mnemonic, operands)?; Instruction::SubtractFromZeroExtended { d, a } }
        "subfe." => { let [d, a, b] = rrr(mnemonic, operands)?; Instruction::SubtractFromExtendedRecord { d, a, b } }
        "or." => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::OrRecord { a, s, b } }
        "xor." => { let [a, s, b] = rrr(mnemonic, operands)?; Instruction::XorRecord { a, s, b } }

        // Rotate-and-mask family. `rlwinm rA,rS,SH,MB,ME` (+ `.` record and the
        // `rlwimi` insert form); the `rotlwi`/`slwi`/`clrlwi` spellings are aliases.
        "rlwinm" => { let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift, begin, end } }
        "rlwinm." => { let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?; Instruction::RotateAndMaskRecord { a, s, shift, begin, end } }
        "rlwimi" => { let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?; Instruction::RotateAndMaskInsert { a, s, shift, begin, end } }
        "rotlwi" => { let (a, s, shift) = rr_shift(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift, begin: 0, end: 31 } }
        "rotrwi" => { let (a, s, n) = rr_shift(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift: (32 - n as u16) as u8 & 31, begin: 0, end: 31 } }
        "slwi" => { let (a, s, shift) = rr_shift(mnemonic, operands)?; Instruction::ShiftLeftImmediate { a, s, shift } }
        "clrlwi" => { let (a, s, clear) = rr_shift(mnemonic, operands)?; Instruction::ClearLeftImmediate { a, s, clear } }
        // More `rlwinm` simplified spellings (all rotate-and-mask with derived fields).
        "srwi" => { let (a, s, n) = rr_shift(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift: (32 - n as u16) as u8 & 31, begin: n, end: 31 } }
        "clrrwi" => { let (a, s, n) = rr_shift(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift: 0, begin: 0, end: 31 - n } }
        "clrrwi." => { let (a, s, n) = rr_shift(mnemonic, operands)?; Instruction::RotateAndMaskRecord { a, s, shift: 0, begin: 0, end: 31 - n } }
        // `extlwi rA,rS,n,b` = rlwinm rA,rS,b,0,n-1 (extract n bits at b, left-justify).
        "extlwi" => { let (a, s, n, b) = rr_two_immediates(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift: b, begin: 0, end: n - 1 } }
        // `extrwi rA,rS,n,b` = rlwinm rA,rS,b+n,32-n,31 (extract n bits at b, right-justify).
        "extrwi" => { let (a, s, n, b) = rr_two_immediates(mnemonic, operands)?; Instruction::RotateAndMask { a, s, shift: (b as u16 + n as u16) as u8 & 31, begin: 32 - n, end: 31 } }

        // Floating point (double): compare (any cr field), subtract, round-to-single,
        // move, and convert-to-integer-word (round toward zero).
        "fcmpu" => {
            let (crf, operands) = take_cr_field(operands);
            let [a, b] = fprs(mnemonic, operands)?;
            if crf == 0 {
                Instruction::FloatCompareUnordered { a, b }
            } else {
                Instruction::FloatCompareUnorderedField { crf, a, b }
            }
        }
        "fsub" => { let [d, a, b] = fprs(mnemonic, operands)?; Instruction::FloatSubtractDouble { d, a, b } }
        "frsp" => { let [d, b] = fprs(mnemonic, operands)?; Instruction::RoundToSingle { d, b } }
        "fmr" => { let [d, b] = fprs(mnemonic, operands)?; Instruction::FloatMove { d, b } }
        "fctiwz" => { let [d, b] = fprs(mnemonic, operands)?; Instruction::ConvertToIntegerWordZero { d, b } }

        // Register + signed-immediate ALU (`op dst, src, SIMM`).
        "addi" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediate { d, a, immediate } }
        "addis" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediateShifted { d, a, immediate } }
        "subfic" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::SubtractFromImmediate { d, a, immediate } }
        // Register + unsigned-immediate logical (`op dst, src, UIMM`).
        "ori" => {
            // The immediate may be a `sym@l` relocation (assembled as 0, patched later).
            expect_operand_count(mnemonic, operands, 3)?;
            let a = gpr(mnemonic, &operands[0])?;
            let s = gpr(mnemonic, &operands[1])?;
            let immediate = unsigned_immediate_or_symbol(mnemonic, &operands[2])?;
            Instruction::OrImmediate { a, s, immediate }
        }
        "oris" => { let (a, s, immediate) = rri_u(mnemonic, operands)?; Instruction::OrImmediateShifted { a, s, immediate } }

        // Integer loads/stores: `op rT, <disp>(rA)`.
        "lwz" => { let (d, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::LoadWord { d, a, offset } }
        "lbz" => { let (d, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::LoadByteZero { d, a, offset } }
        "lhz" => { let (d, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::LoadHalfwordZero { d, a, offset } }
        "stw" => { let (s, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::StoreWord { s, a, offset } }
        "stwu" => { let (s, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::StoreWordWithUpdate { s, a, offset } }
        "stb" => { let (s, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::StoreByte { s, a, offset } }
        "sth" => { let (s, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::StoreHalfword { s, a, offset } }
        // Floating loads/stores: `op fT, <disp>(rA)`.
        "lfd" => { let (d, offset, a) = fpr_mem(mnemonic, operands)?; Instruction::LoadFloatDouble { d, a, offset } }
        "lfs" => { let (d, offset, a) = fpr_mem(mnemonic, operands)?; Instruction::LoadFloatSingle { d, a, offset } }
        "stfd" => { let (s, offset, a) = fpr_mem(mnemonic, operands)?; Instruction::StoreFloatDouble { s, a, offset } }
        "stfs" => { let (s, offset, a) = fpr_mem(mnemonic, operands)?; Instruction::StoreFloatSingle { s, a, offset } }

        // Compares with an optional explicit condition field. `cmpwi` handles any
        // `crN`; the others model cr0 only (a non-cr0 field DEFERS). `cmpi`/`cmpli`
        // are the older mnemonic spellings.
        "cmpwi" => {
            let (crf, operands) = take_cr_field(operands);
            let (a, immediate) = gpr_immediate(mnemonic, operands)?;
            if crf == 0 {
                Instruction::CompareWordImmediate { a, immediate }
            } else {
                Instruction::CompareWordImmediateField { crf, a, immediate }
            }
        }
        // `cmpi crfD, L, rA, SIMM` — the classic four-operand spelling (L = 0 for a
        // 32-bit compare; L != 0 defers).
        "cmpi" => {
            let (crf, operands) = strip_length_bit(mnemonic, take_cr_field(operands))?;
            let (a, immediate) = gpr_immediate(mnemonic, operands)?;
            if crf == 0 {
                Instruction::CompareWordImmediate { a, immediate }
            } else {
                Instruction::CompareWordImmediateField { crf, a, immediate }
            }
        }
        "cmplwi" => {
            let operands = require_cr0(mnemonic, operands)?;
            expect_operand_count(mnemonic, operands, 2)?;
            let a = gpr(mnemonic, &operands[0])?;
            let immediate = immediate16u(mnemonic, &operands[1])?;
            Instruction::CompareLogicalWordImmediate { a, immediate }
        }
        // `cmpli crfD, L, rA, UIMM` — classic four-operand spelling (cr0/L=0 only).
        "cmpli" => {
            let (crf, operands) = strip_length_bit(mnemonic, take_cr_field(operands))?;
            if crf != 0 {
                return Err(Diagnostic::error("inline-asm 'cmpli' on a non-cr0 field is not supported yet (roadmap)"));
            }
            expect_operand_count(mnemonic, operands, 2)?;
            let a = gpr(mnemonic, &operands[0])?;
            let immediate = immediate16u(mnemonic, &operands[1])?;
            Instruction::CompareLogicalWordImmediate { a, immediate }
        }
        "cmpw" => { let operands = require_cr0(mnemonic, operands)?; let [a, b] = gprs(mnemonic, operands)?; Instruction::CompareWord { a, b } }
        "cmplw" => { let operands = require_cr0(mnemonic, operands)?; let [a, b] = gprs(mnemonic, operands)?; Instruction::CompareLogicalWord { a, b } }

        // The count register (`bdnz` loop support).
        "mtctr" => { let [s] = gprs(mnemonic, operands)?; Instruction::MoveToCountRegister { s } }

        // The link register, condition register, and FPSCR moves + the multi-word
        // load/store — the setjmp/longjmp register-save vocabulary (Gecko_setjmp.c).
        "mflr" => { let [d] = gprs(mnemonic, operands)?; Instruction::MoveFromLinkRegister { d } }
        "mtlr" => { let [s] = gprs(mnemonic, operands)?; Instruction::MoveToLinkRegister { s } }
        "mfcr" => { let [d] = gprs(mnemonic, operands)?; Instruction::MoveFromConditionRegister { d } }
        "mffs" => { let [d] = fprs(mnemonic, operands)?; Instruction::MoveFromFpscr { d } }
        // `mtcrf CRM, rS` / `mtfsf FM, frB` — an 8-bit field mask then the source.
        "mtcrf" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let mask = immediate16u(mnemonic, &operands[0])?;
            let mask = u8::try_from(mask).map_err(|_| Diagnostic::error(format!("{mnemonic} field mask {mask} does not fit in 8 bits")))?;
            let [s] = gprs(mnemonic, &operands[1..])?;
            Instruction::MoveToConditionRegisterFields { mask, s }
        }
        "mtfsf" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let mask = immediate16u(mnemonic, &operands[0])?;
            let mask = u8::try_from(mask).map_err(|_| Diagnostic::error(format!("{mnemonic} field mask {mask} does not fit in 8 bits")))?;
            let [b] = fprs(mnemonic, &operands[1..])?;
            Instruction::MoveToFpscrFields { mask, b }
        }
        "stmw" => { let (s, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::StoreMultipleWord { s, a, offset } }
        "lmw" => { let (d, offset, a) = gpr_mem(mnemonic, operands)?; Instruction::LoadMultipleWord { d, a, offset } }

        // Conditional branch-to-link (a conditional return, `bgtlr` etc.); an
        // optional leading `crN` selects the field. Written directly by mwcc's asm
        // (distinct from the branch-to-`blr` peephole).
        "beqlr" | "bnelr" | "bltlr" | "bgelr" | "bgtlr" | "blelr" => {
            if hint != BranchHint::None {
                return Err(Diagnostic::error("inline-asm branch-to-link with a prediction hint is not supported yet (roadmap)"));
            }
            let base = &mnemonic[..mnemonic.len() - 2]; // strip the `lr`
            let (base_options, base_bit) = conditional_branch_fields(base);
            let (crf, operands) = take_cr_field(operands);
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchConditionalToLinkRegister { options: base_options, condition_bit: crf * 4 + base_bit }
        }
        // Unconditional branch to a label.
        "b" => Instruction::Branch { target: label_target(mnemonic, operands, labels)? },
        // Conditional branches; an optional leading `crN` selects the condition
        // field (`BI = crN*4 + bit`). The target is a label.
        "beq" | "bne" | "blt" | "bge" | "bgt" | "ble" | "bdnz" => {
            let (base_options, base_bit) = conditional_branch_fields(mnemonic);
            let (crf, operands) = take_cr_field(operands);
            let condition_bit = crf * 4 + base_bit;
            let target = label_target(mnemonic, operands, labels)?;
            let forward = target >= instruction_index;
            let options = base_options | hint_bit(hint, forward);
            Instruction::BranchConditionalForward { options, condition_bit, target }
        }

        other => return Err(Diagnostic::error(format!("inline-asm mnemonic '{other}' is not supported yet (roadmap)"))),
    };
    Ok(Some(instruction))
}

/// A branch static-prediction hint parsed from a `+`/`-` mnemonic suffix.
#[derive(Clone, Copy, PartialEq)]
enum BranchHint {
    None,
    Taken,
    NotTaken,
}

/// The BO "y" (prediction) bit for a hinted branch. The default prediction is
/// implied by direction (backward = taken, forward = not taken); the bit is set
/// only when the requested prediction differs from that default.
fn hint_bit(hint: BranchHint, forward: bool) -> u8 {
    match hint {
        BranchHint::None => 0,
        BranchHint::Taken => u8::from(forward),
        BranchHint::NotTaken => u8::from(!forward),
    }
}

/// The `(BO, BI)` fields for a cr0 conditional branch mnemonic.
fn conditional_branch_fields(mnemonic: &str) -> (u8, u8) {
    match mnemonic {
        "beq" => (12, 2),
        "bne" => (4, 2),
        "blt" => (12, 0),
        "bge" => (4, 0),
        "bgt" => (12, 1),
        "ble" => (4, 1),
        // `bdnz`: decrement CTR, branch if CTR != 0 (BO = 16, BI ignored).
        "bdnz" => (16, 0),
        _ => unreachable!("conditional_branch_fields called with '{mnemonic}'"),
    }
}

/// Resolve a branch's single label operand to its instruction index.
fn label_target(mnemonic: &str, operands: &[AsmOperand], labels: &HashMap<&str, usize>) -> Compilation<usize> {
    expect_operand_count(mnemonic, operands, 1)?;
    match &operands[0] {
        AsmOperand::Label(name) => labels
            .get(name.as_str())
            .copied()
            .ok_or_else(|| Diagnostic::error(format!("inline-asm branch to undefined label '{name}'"))),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected a label operand"))),
    }
}
