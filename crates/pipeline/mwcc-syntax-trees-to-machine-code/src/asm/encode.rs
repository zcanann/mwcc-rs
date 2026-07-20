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
pub(super) fn assemble_line(
    line: &AsmInstruction,
    labels: &HashMap<&str, usize>,
    instruction_index: usize,
) -> Compilation<Option<Instruction>> {
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
        "add" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::Add { d, a, b }
        }
        "subf" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::SubtractFrom { d, a, b }
        }
        // `sub rD, rA, rB` (rD = rA - rB) is the simplified spelling of `subf rD, rB, rA`.
        "sub" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::SubtractFrom { d, a: b, b: a }
        }
        "subfc" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::SubtractFromCarrying { d, a, b }
        }
        "subfe" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::SubtractFromExtended { d, a, b }
        }
        "adde" => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::AddExtended { d, a, b }
        }
        "or" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::Or { a, s, b }
        }
        "and" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::And { a, s, b }
        }
        "xor" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::Xor { a, s, b }
        }
        "nor" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::Nor { a, s, b }
        }
        "slw" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::ShiftLeftWord { a, s, b }
        }
        "srw" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::ShiftRightWord { a, s, b }
        }

        // Two-register ALU (`op dst, src`).
        "neg" => {
            let [d, a] = gprs(mnemonic, operands)?;
            Instruction::Negate { d, a }
        }
        "cntlzw" => {
            let [a, s] = gprs(mnemonic, operands)?;
            Instruction::CountLeadingZeros { a, s }
        }
        "sraw" => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::ShiftRightAlgebraicWord { a, s, b }
        }
        "srawi" => {
            let (a, s, shift) = rr_shift(mnemonic, operands)?;
            Instruction::ShiftRightAlgebraicImmediate { a, s, shift }
        }
        "addze" => {
            let [d, a] = gprs(mnemonic, operands)?;
            Instruction::AddToZeroExtended { d, a }
        }

        // Add-immediate-carrying (`addic dst, src, SIMM`); the simplified `subic`
        // spelling negates the immediate (`subic d, a, v` == `addic d, a, -v`).
        "addic" => {
            let (d, a, immediate) = rri(mnemonic, operands)?;
            Instruction::AddImmediateCarrying { d, a, immediate }
        }
        "subic" => {
            let (d, a, immediate) = rri(mnemonic, operands)?;
            let immediate = immediate.checked_neg().ok_or_else(|| {
                Diagnostic::error("inline-asm 'subic' immediate overflows on negation")
            })?;
            Instruction::AddImmediateCarrying { d, a, immediate }
        }
        "addic." => {
            let (d, a, immediate) = rri(mnemonic, operands)?;
            Instruction::AddImmediateCarryingRecord { d, a, immediate }
        }
        "subfze" => {
            let [d, a] = gprs(mnemonic, operands)?;
            Instruction::SubtractFromZeroExtended { d, a }
        }
        "subfe." => {
            let [d, a, b] = rrr(mnemonic, operands)?;
            Instruction::SubtractFromExtendedRecord { d, a, b }
        }
        "or." => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::OrRecord { a, s, b }
        }
        "xor." => {
            let [a, s, b] = rrr(mnemonic, operands)?;
            Instruction::XorRecord { a, s, b }
        }

        // Rotate-and-mask family. `rlwinm rA,rS,SH,MB,ME` (+ `.` record and the
        // `rlwimi` insert form); the `rotlwi`/`slwi`/`clrlwi` spellings are aliases.
        "rlwinm" => {
            let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift,
                begin,
                end,
            }
        }
        "rlwinm." => {
            let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?;
            Instruction::RotateAndMaskRecord {
                a,
                s,
                shift,
                begin,
                end,
            }
        }
        "rlwimi" => {
            let (a, s, shift, begin, end) = rotate5(mnemonic, operands)?;
            Instruction::RotateAndMaskInsert {
                a,
                s,
                shift,
                begin,
                end,
            }
        }
        "rotlwi" => {
            let (a, s, shift) = rr_shift(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift,
                begin: 0,
                end: 31,
            }
        }
        "rotrwi" => {
            let (a, s, n) = rr_shift(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift: (32 - n as u16) as u8 & 31,
                begin: 0,
                end: 31,
            }
        }
        "slwi" => {
            let (a, s, shift) = rr_shift(mnemonic, operands)?;
            Instruction::ShiftLeftImmediate { a, s, shift }
        }
        "clrlwi" => {
            let (a, s, clear) = rr_shift(mnemonic, operands)?;
            Instruction::ClearLeftImmediate { a, s, clear }
        }
        "clrlwi." => {
            let (a, s, clear) = rr_shift(mnemonic, operands)?;
            Instruction::ClearLeftImmediateRecord { a, s, clear }
        }
        // `nop` = `ori r0, r0, 0`.
        "nop" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: 0,
            }
        }
        // More `rlwinm` simplified spellings (all rotate-and-mask with derived fields).
        "srwi" => {
            let (a, s, n) = rr_shift(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift: (32 - n as u16) as u8 & 31,
                begin: n,
                end: 31,
            }
        }
        "clrrwi" => {
            let (a, s, n) = rr_shift(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift: 0,
                begin: 0,
                end: 31 - n,
            }
        }
        "clrrwi." => {
            let (a, s, n) = rr_shift(mnemonic, operands)?;
            Instruction::RotateAndMaskRecord {
                a,
                s,
                shift: 0,
                begin: 0,
                end: 31 - n,
            }
        }
        // `extlwi rA,rS,n,b` = rlwinm rA,rS,b,0,n-1 (extract n bits at b, left-justify).
        "extlwi" => {
            let (a, s, n, b) = rr_two_immediates(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift: b,
                begin: 0,
                end: n - 1,
            }
        }
        // `extrwi rA,rS,n,b` = rlwinm rA,rS,b+n,32-n,31 (extract n bits at b, right-justify).
        "extrwi" => {
            let (a, s, n, b) = rr_two_immediates(mnemonic, operands)?;
            Instruction::RotateAndMask {
                a,
                s,
                shift: (b as u16 + n as u16) as u8 & 31,
                begin: 32 - n,
                end: 31,
            }
        }

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
        "fsub" => {
            let [d, a, b] = fprs(mnemonic, operands)?;
            Instruction::FloatSubtractDouble { d, a, b }
        }
        "frsp" => {
            let [d, b] = fprs(mnemonic, operands)?;
            Instruction::RoundToSingle { d, b }
        }
        "fmr" => {
            let [d, b] = fprs(mnemonic, operands)?;
            Instruction::FloatMove { d, b }
        }
        "fctiwz" => {
            let [d, b] = fprs(mnemonic, operands)?;
            Instruction::ConvertToIntegerWordZero { d, b }
        }

        // Register + signed-immediate ALU (`op dst, src, SIMM`).
        "addi" => {
            let (d, a, immediate) = rri_symbolic(mnemonic, operands)?;
            Instruction::AddImmediate { d, a, immediate }
        }
        "addis" => {
            let (d, a, immediate) = rri_symbolic(mnemonic, operands)?;
            Instruction::AddImmediateShifted { d, a, immediate }
        }
        "subfic" => {
            let (d, a, immediate) = rri(mnemonic, operands)?;
            Instruction::SubtractFromImmediate { d, a, immediate }
        }
        // Register + unsigned-immediate logical (`op dst, src, UIMM`).
        "ori" => {
            // The immediate may be a `sym@l` relocation (assembled as 0, patched later).
            expect_operand_count(mnemonic, operands, 3)?;
            let a = gpr(mnemonic, &operands[0])?;
            let s = gpr(mnemonic, &operands[1])?;
            let immediate = unsigned_immediate_or_symbol(mnemonic, &operands[2])?;
            Instruction::OrImmediate { a, s, immediate }
        }
        "oris" => {
            let (a, s, immediate) = rri_u(mnemonic, operands)?;
            Instruction::OrImmediateShifted { a, s, immediate }
        }
        "xori" => {
            let (a, s, immediate) = rri_u(mnemonic, operands)?;
            Instruction::XorImmediate { a, s, immediate }
        }
        "xoris" => {
            let (a, s, immediate) = rri_u(mnemonic, operands)?;
            Instruction::XorImmediateShifted { a, s, immediate }
        }

        // Integer loads/stores: `op rT, <disp>(rA)`.
        "lwz" => {
            let (d, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::LoadWord { d, a, offset }
        }
        "lbz" => {
            let (d, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::LoadByteZero { d, a, offset }
        }
        "lhz" => {
            let (d, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::LoadHalfwordZero { d, a, offset }
        }
        "stw" => {
            let (s, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::StoreWord { s, a, offset }
        }
        "stwu" => {
            let (s, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::StoreWordWithUpdate { s, a, offset }
        }
        "stb" => {
            let (s, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::StoreByte { s, a, offset }
        }
        "sth" => {
            let (s, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::StoreHalfword { s, a, offset }
        }
        // Floating loads/stores: `op fT, <disp>(rA)`.
        "lfd" => {
            let (d, offset, a) = fpr_mem(mnemonic, operands)?;
            Instruction::LoadFloatDouble { d, a, offset }
        }
        "lfs" => {
            let (d, offset, a) = fpr_mem(mnemonic, operands)?;
            Instruction::LoadFloatSingle { d, a, offset }
        }
        "stfd" => {
            let (s, offset, a) = fpr_mem(mnemonic, operands)?;
            Instruction::StoreFloatDouble { s, a, offset }
        }
        "stfs" => {
            let (s, offset, a) = fpr_mem(mnemonic, operands)?;
            Instruction::StoreFloatSingle { s, a, offset }
        }
        "psq_l" => {
            let (d, offset, a, w, i) = quantized_fpr_mem(mnemonic, operands)?;
            Instruction::PairedSingleQuantizedLoad { d, a, offset, w, i }
        }
        "psq_st" => {
            let (s, offset, a, w, i) = quantized_fpr_mem(mnemonic, operands)?;
            Instruction::PairedSingleQuantizedStore { s, a, offset, w, i }
        }

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
                return Err(Diagnostic::error(
                    "inline-asm 'cmpli' on a non-cr0 field is not supported yet (roadmap)",
                ));
            }
            expect_operand_count(mnemonic, operands, 2)?;
            let a = gpr(mnemonic, &operands[0])?;
            let immediate = immediate16u(mnemonic, &operands[1])?;
            Instruction::CompareLogicalWordImmediate { a, immediate }
        }
        "cmpw" => {
            // cr0 keeps the plain form; a `crN` field selects the field compare
            // (`cmpw cr6, rA, rB` — the BfBB ptmf vtable test).
            let (crf, operands) = take_cr_field(operands);
            let [a, b] = gprs(mnemonic, operands)?;
            if crf == 0 {
                Instruction::CompareWord { a, b }
            } else {
                Instruction::CompareWordField { crf, a, b }
            }
        }
        "cmplw" => {
            let operands = require_cr0(mnemonic, operands)?;
            let [a, b] = gprs(mnemonic, operands)?;
            Instruction::CompareLogicalWord { a, b }
        }

        // The count register (`bdnz` loop support).
        "mtctr" => {
            let [s] = gprs(mnemonic, operands)?;
            Instruction::MoveToCountRegister { s }
        }
        "mfctr" => {
            let [d] = gprs(mnemonic, operands)?;
            // `mfctr rD` is the dedicated assembler spelling of `mfspr rD,9`.
            // The generic SPR instruction retains the exact encoding without
            // introducing a second IR form used only by verbatim asm bodies.
            Instruction::MoveFromSpr { d, spr: 9 }
        }

        // Special-purpose / machine-state register moves + the synchronization and
        // interrupt-return system ops — the OS-kernel inline-asm vocabulary
        // (OSCache/OSReset/OSSync/OSReboot). The SPR operand accepts a number or a
        // named register (GQR0-7, HID0-2, the time base, …).
        "mfspr" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let d = gpr(mnemonic, &operands[0])?;
            let spr = special_register(mnemonic, &operands[1])?;
            Instruction::MoveFromSpr { d, spr }
        }
        "mtspr" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let spr = special_register(mnemonic, &operands[0])?;
            let s = gpr(mnemonic, &operands[1])?;
            Instruction::MoveToSpr { spr, s }
        }
        "mfsr" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let d = gpr(mnemonic, &operands[0])?;
            let segment = immediate16u(mnemonic, &operands[1])?;
            let segment = u8::try_from(segment)
                .ok()
                .filter(|segment| *segment < 16)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "inline-asm '{mnemonic}' segment register must be 0..=15"
                    ))
                })?;
            Instruction::MoveFromSegmentRegister { d, segment }
        }
        "mtsr" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let segment = immediate16u(mnemonic, &operands[0])?;
            let segment = u8::try_from(segment)
                .ok()
                .filter(|segment| *segment < 16)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "inline-asm '{mnemonic}' segment register must be 0..=15"
                    ))
                })?;
            let s = gpr(mnemonic, &operands[1])?;
            Instruction::MoveToSegmentRegister { segment, s }
        }
        // Dedicated architecture spellings are aliases for fixed SPR numbers.
        // Keeping them as structured SPR moves shares encoding and register-use
        // semantics with the explicit `mfspr`/`mtspr` forms.
        "mfxer" | "mfpvr" | "mfdar" | "mfdsisr" | "mfdec" | "mfsdr1" | "mfear" => {
            let [d] = gprs(mnemonic, operands)?;
            let spr = match mnemonic {
                "mfxer" => 1,
                "mfpvr" => 287,
                "mfdar" => 19,
                "mfdsisr" => 18,
                "mfdec" => 22,
                "mfsdr1" => 25,
                "mfear" => 282,
                _ => unreachable!(),
            };
            Instruction::MoveFromSpr { d, spr }
        }
        "mfibatu" | "mfibatl" | "mfdbatu" | "mfdbatl" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let d = gpr(mnemonic, &operands[0])?;
            let index = immediate16u(mnemonic, &operands[1])?;
            if index > 3 {
                return Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' BAT index must be 0..=3"
                )));
            }
            let base = if mnemonic.starts_with("mfibat") {
                528
            } else {
                536
            };
            let lower = u16::from(mnemonic.ends_with('l'));
            Instruction::MoveFromSpr {
                d,
                spr: base + index * 2 + lower,
            }
        }
        "mfsprg" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let d = gpr(mnemonic, &operands[0])?;
            let index = immediate16u(mnemonic, &operands[1])?;
            if index > 3 {
                return Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' SPRG index must be 0..=3"
                )));
            }
            Instruction::MoveFromSpr {
                d,
                spr: 272 + index,
            }
        }
        "mtibatu" | "mtibatl" | "mtdbatu" | "mtdbatl" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let index = immediate16u(mnemonic, &operands[0])?;
            if index > 3 {
                return Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' BAT index must be 0..=3"
                )));
            }
            let s = gpr(mnemonic, &operands[1])?;
            let base = if mnemonic.starts_with("mtibat") {
                528
            } else {
                536
            };
            let lower = u16::from(mnemonic.ends_with('l'));
            Instruction::MoveToSpr {
                spr: base + index * 2 + lower,
                s,
            }
        }
        "mtxer" | "mtdar" | "mtdsisr" | "mtdec" | "mtsdr1" | "mtear" => {
            let [s] = gprs(mnemonic, operands)?;
            let spr = match mnemonic {
                "mtxer" => 1,
                "mtdar" => 19,
                "mtdsisr" => 18,
                "mtdec" => 22,
                "mtsdr1" => 25,
                "mtear" => 282,
                _ => unreachable!(),
            };
            Instruction::MoveToSpr { spr, s }
        }
        "mtsprg" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let index = immediate16u(mnemonic, &operands[0])?;
            if index > 3 {
                return Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' SPRG index must be 0..=3"
                )));
            }
            let s = gpr(mnemonic, &operands[1])?;
            Instruction::MoveToSpr {
                spr: 272 + index,
                s,
            }
        }
        "mttbl" | "mttbu" => {
            let [s] = gprs(mnemonic, operands)?;
            let spr = if mnemonic == "mttbl" { 284 } else { 285 };
            Instruction::MoveToSpr { spr, s }
        }
        "mftb" => {
            if !matches!(operands.len(), 1 | 2) {
                return Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' expected 1 or 2 operand(s), found {}",
                    operands.len()
                )));
            }
            let d = gpr(mnemonic, &operands[0])?;
            let spr = if operands.len() == 2 {
                special_register(mnemonic, &operands[1])?
            } else {
                268
            };
            Instruction::MoveFromTimeBase { d, tbr: spr }
        }
        "mftbu" => {
            let [d] = gprs(mnemonic, operands)?;
            Instruction::MoveFromTimeBase { d, tbr: 269 }
        }
        "mfmsr" => {
            let [d] = gprs(mnemonic, operands)?;
            Instruction::MoveFromMsr { d }
        }
        "mtmsr" => {
            let [s] = gprs(mnemonic, operands)?;
            Instruction::MoveToMsr { s }
        }
        // Dedicated spellings for the save/restore registers are assembler aliases for SPR
        // 26/27. Keep them as structured SPR moves so encoding and register-use analysis share
        // the same representation as an explicit `mtspr`/`mfspr`.
        "mfsrr0" | "mfsrr1" => {
            let [d] = gprs(mnemonic, operands)?;
            let spr = if mnemonic == "mfsrr0" { 26 } else { 27 };
            Instruction::MoveFromSpr { d, spr }
        }
        "mtsrr0" | "mtsrr1" => {
            let [s] = gprs(mnemonic, operands)?;
            let spr = if mnemonic == "mtsrr0" { 26 } else { 27 };
            Instruction::MoveToSpr { spr, s }
        }
        "isync" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::InstructionSynchronize
        }
        "sync" | "hwsync" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::Synchronize
        }
        "eieio" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::EnforceInOrderIo
        }
        "rfi" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::ReturnFromInterrupt
        }
        "sc" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::SystemCall
        }

        // Cache-block ops (`op rA, rB`, addressing `(rA|0) + rB`). `dcbz_l` is the
        // Gekko locked-cache variant (primary opcode 4); the rest are opcode 31.
        "dcbst" | "dcbf" | "dcbt" | "dcbtst" | "dcbi" | "dcbz" | "icbi" | "dcbz_l" => {
            // `dcbt 0, rB` writes the base as the literal 0 (an immediate), not r0.
            expect_operand_count(mnemonic, operands, 2)?;
            let a = gpr_or_zero(mnemonic, &operands[0])?;
            let b = gpr(mnemonic, &operands[1])?;
            let (primary, xo) = match mnemonic {
                "dcbst" => (31, 54),
                "dcbf" => (31, 86),
                "dcbt" => (31, 278),
                "dcbtst" => (31, 246),
                "dcbi" => (31, 470),
                "dcbz" => (31, 1014),
                "icbi" => (31, 982),
                "dcbz_l" => (4, 1014),
                _ => unreachable!(),
            };
            Instruction::CacheOp { primary, xo, a, b }
        }

        // The link register, condition register, and FPSCR moves + the multi-word
        // load/store — the setjmp/longjmp register-save vocabulary (Gecko_setjmp.c).
        "mflr" => {
            let [d] = gprs(mnemonic, operands)?;
            Instruction::MoveFromLinkRegister { d }
        }
        "mtlr" => {
            let [s] = gprs(mnemonic, operands)?;
            Instruction::MoveToLinkRegister { s }
        }
        "mfcr" => {
            let [d] = gprs(mnemonic, operands)?;
            Instruction::MoveFromConditionRegister { d }
        }
        "mtcr" => {
            let [s] = gprs(mnemonic, operands)?;
            Instruction::MoveToConditionRegisterFields { mask: 0xff, s }
        }
        "mffs" => {
            let [d] = fprs(mnemonic, operands)?;
            Instruction::MoveFromFpscr { d }
        }
        // `mtcrf CRM, rS` / `mtfsf FM, frB` — an 8-bit field mask then the source.
        "mtcrf" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let mask = immediate16u(mnemonic, &operands[0])?;
            let mask = u8::try_from(mask).map_err(|_| {
                Diagnostic::error(format!(
                    "{mnemonic} field mask {mask} does not fit in 8 bits"
                ))
            })?;
            let [s] = gprs(mnemonic, &operands[1..])?;
            Instruction::MoveToConditionRegisterFields { mask, s }
        }
        "mtfsf" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let mask = immediate16u(mnemonic, &operands[0])?;
            let mask = u8::try_from(mask).map_err(|_| {
                Diagnostic::error(format!(
                    "{mnemonic} field mask {mask} does not fit in 8 bits"
                ))
            })?;
            let [b] = fprs(mnemonic, &operands[1..])?;
            Instruction::MoveToFpscrFields { mask, b }
        }
        "stmw" => {
            let (s, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::StoreMultipleWord { s, a, offset }
        }
        "lmw" => {
            let (d, offset, a) = gpr_mem(mnemonic, operands)?;
            Instruction::LoadMultipleWord { d, a, offset }
        }

        // Conditional branch-to-link (a conditional return, `bgtlr` etc.); an
        // optional leading `crN` selects the field. Written directly by mwcc's asm
        // (distinct from the branch-to-`blr` peephole).
        "beqlr" | "bnelr" | "bltlr" | "bgelr" | "bgtlr" | "blelr" => {
            // A prediction hint on a branch-to-link is DROPPED by mwcc (measured:
            // `bnelr-` assembles to the plain 4c 82 00 20, y = 0), so it is accepted
            // and ignored — unlike a displacement branch, where the y bit is real.
            let _ = hint;
            let base = &mnemonic[..mnemonic.len() - 2]; // strip the `lr`
            let (base_options, base_bit) = conditional_branch_fields(base);
            let (crf, operands) = take_cr_field(operands);
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchConditionalToLinkRegister {
                options: base_options,
                condition_bit: crf * 4 + base_bit,
            }
        }
        // Indexed word load (`lwzx rD, rA, rB` — the ptmf vtable dispatch).
        "lwzx" => {
            let [d, a, b] = gprs(mnemonic, operands)?;
            Instruction::LoadWordIndexed { d, a, b }
        }
        // Branch to the count register (`mtctr r12; bctr` — the ptmf tail dispatch).
        "bctr" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchToCountRegister
        }
        "bctrl" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchToCountRegisterAndLink
        }
        "blrl" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchToLinkRegisterAndLink
        }
        // Unconditional branch to a label, or a tail branch to an external symbol.
        "b" => {
            expect_operand_count(mnemonic, operands, 1)?;
            match &operands[0] {
                // A local label resolves to its instruction index.
                AsmOperand::Label(name) if labels.contains_key(name.as_str()) => {
                    Instruction::Branch {
                        target: labels[name.as_str()],
                    }
                }
                // A name with no local label is a tail branch to an external
                // function (`b func`): an offset-0 placeholder (`48 00 00 00`)
                // patched by the `R_PPC_REL24` relocation recorded in `mod.rs`.
                AsmOperand::Label(_) => Instruction::Branch {
                    target: instruction_index,
                },
                _ => {
                    return Err(Diagnostic::error(format!(
                        "inline-asm '{mnemonic}' expected a label operand"
                    )))
                }
            }
        }
        // Direct call to an external symbol. A local-label `bl` needs a position-resolved linked
        // branch variant; no measured source uses it yet, so keep that distinct shape deferred.
        "bl" => {
            expect_operand_count(mnemonic, operands, 1)?;
            match &operands[0] {
                AsmOperand::Label(name) if !labels.contains_key(name.as_str()) => {
                    Instruction::BranchAndLink {
                        target: name.clone(),
                    }
                }
                AsmOperand::Label(name) => {
                    return Err(Diagnostic::error(format!(
                        "inline-asm local linked branch to '{name}' is not supported yet (roadmap)"
                    )))
                }
                _ => {
                    return Err(Diagnostic::error(
                        "inline-asm 'bl' expected a label operand",
                    ))
                }
            }
        }
        // Conditional branches; an optional leading `crN` selects the condition
        // field (`BI = crN*4 + bit`). The target is a label.
        "beq" | "bne" | "blt" | "bge" | "bgt" | "ble" | "bdnz" => {
            let (base_options, base_bit) = conditional_branch_fields(mnemonic);
            let (crf, operands) = take_cr_field(operands);
            let condition_bit = crf * 4 + base_bit;
            let target = label_target(mnemonic, operands, labels)?;
            let forward = target >= instruction_index;
            let options = base_options | hint_bit(hint, forward);
            Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target,
            }
        }
        // Raw conditional-branch spelling: `bc BO, BI, label`.
        "bc" => {
            expect_operand_count(mnemonic, operands, 3)?;
            let branch_field = |operand: &AsmOperand, name: &str| match operand {
                AsmOperand::Immediate(value @ 0..=31) => Ok(*value as u8),
                _ => Err(Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' {name} field must be 0..=31"
                ))),
            };
            let options = branch_field(&operands[0], "BO")?;
            let condition_bit = branch_field(&operands[1], "BI")?;
            let target = label_target(mnemonic, &operands[2..], labels)?;
            Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target,
            }
        }

        other => {
            return Err(Diagnostic::error(format!(
                "inline-asm mnemonic '{other}' is not supported yet (roadmap)"
            )))
        }
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
fn label_target(
    mnemonic: &str,
    operands: &[AsmOperand],
    labels: &HashMap<&str, usize>,
) -> Compilation<usize> {
    expect_operand_count(mnemonic, operands, 1)?;
    match &operands[0] {
        AsmOperand::Label(name) => labels.get(name.as_str()).copied().ok_or_else(|| {
            Diagnostic::error(format!("inline-asm branch to undefined label '{name}'"))
        }),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected a label operand"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assemble(mnemonic: &str, operands: Vec<AsmOperand>) -> Compilation<Instruction> {
        assemble_line(
            &AsmInstruction {
                mnemonic: mnemonic.to_string(),
                operands,
                source_line: 1,
            },
            &HashMap::new(),
            0,
        )?
        .ok_or_else(|| Diagnostic::error("test instruction emitted no word"))
    }

    #[test]
    fn assembles_segment_register_moves() {
        assert_eq!(
            assemble(
                "mfsr",
                vec![AsmOperand::Gpr(16), AsmOperand::Immediate(0)]
            )
            .unwrap(),
            Instruction::MoveFromSegmentRegister { d: 16, segment: 0 }
        );
        assert_eq!(
            assemble(
                "mtsr",
                vec![AsmOperand::Immediate(15), AsmOperand::Gpr(31)]
            )
            .unwrap(),
            Instruction::MoveToSegmentRegister { segment: 15, s: 31 }
        );
        assert!(assemble(
            "mfsr",
            vec![AsmOperand::Gpr(3), AsmOperand::Immediate(16)]
        )
        .is_err());
    }

    #[test]
    fn assembles_privileged_spr_aliases() {
        assert_eq!(
            assemble("mfctr", vec![AsmOperand::Gpr(0)]).unwrap(),
            Instruction::MoveFromSpr { d: 0, spr: 9 }
        );
        assert_eq!(
            assemble("mfxer", vec![AsmOperand::Gpr(31)]).unwrap(),
            Instruction::MoveFromSpr { d: 31, spr: 1 }
        );
        assert_eq!(
            assemble("mtxer", vec![AsmOperand::Gpr(4)]).unwrap(),
            Instruction::MoveToSpr { spr: 1, s: 4 }
        );
        assert_eq!(
            assemble(
                "mfdbatl",
                vec![AsmOperand::Gpr(25), AsmOperand::Immediate(0)]
            )
            .unwrap(),
            Instruction::MoveFromSpr { d: 25, spr: 537 }
        );
        assert_eq!(
            assemble(
                "mtibatu",
                vec![AsmOperand::Immediate(3), AsmOperand::Gpr(22)]
            )
            .unwrap(),
            Instruction::MoveToSpr { spr: 534, s: 22 }
        );
        assert_eq!(
            assemble(
                "mfsprg",
                vec![AsmOperand::Gpr(27), AsmOperand::Immediate(2)]
            )
            .unwrap(),
            Instruction::MoveFromSpr { d: 27, spr: 274 }
        );
        assert_eq!(
            assemble("mtsdr1", vec![AsmOperand::Gpr(22)]).unwrap(),
            Instruction::MoveToSpr { spr: 25, s: 22 }
        );
    }

    #[test]
    fn assembles_condition_register_aliases() {
        assert_eq!(
            assemble("mtcr", vec![AsmOperand::Gpr(4)]).unwrap(),
            Instruction::MoveToConditionRegisterFields { mask: 0xff, s: 4 }
        );
    }

    #[test]
    fn assembles_indirect_linked_branches() {
        assert_eq!(
            assemble("bctrl", vec![]).unwrap(),
            Instruction::BranchToCountRegisterAndLink
        );
        assert_eq!(
            assemble("blrl", vec![]).unwrap(),
            Instruction::BranchToLinkRegisterAndLink
        );
    }

    #[test]
    fn assembles_relocated_add_immediates() {
        assert_eq!(
            assemble(
                "addi",
                vec![
                    AsmOperand::Gpr(5),
                    AsmOperand::Gpr(4),
                    AsmOperand::Symbol {
                        name: "target".to_string(),
                        suffix: mwcc_syntax_trees::AsmRelocSuffix::Lo,
                    },
                ],
            )
            .unwrap(),
            Instruction::AddImmediate {
                d: 5,
                a: 4,
                immediate: 0,
            }
        );
    }

    #[test]
    fn assembles_quantized_paired_single_memory_operations() {
        let operands = vec![
            AsmOperand::Fpr(31),
            AsmOperand::Memory {
                displacement: -16,
                base: 3,
            },
            AsmOperand::Immediate(1),
            AsmOperand::Immediate(7),
        ];
        assert_eq!(
            assemble("psq_l", operands.clone()).unwrap(),
            Instruction::PairedSingleQuantizedLoad {
                d: 31,
                a: 3,
                offset: -16,
                w: 1,
                i: 7,
            }
        );
        assert_eq!(
            assemble("psq_st", operands).unwrap(),
            Instruction::PairedSingleQuantizedStore {
                s: 31,
                a: 3,
                offset: -16,
                w: 1,
                i: 7,
            }
        );
        assert!(assemble(
            "psq_l",
            vec![
                AsmOperand::Fpr(0),
                AsmOperand::Memory {
                    displacement: 2048,
                    base: 3,
                },
                AsmOperand::Immediate(0),
                AsmOperand::Immediate(0),
            ],
        )
        .is_err());
    }

    #[test]
    fn assembles_raw_conditional_branch_fields() {
        let mut labels = HashMap::new();
        labels.insert("done", 9);
        let line = AsmInstruction {
            mnemonic: "bc".to_string(),
            operands: vec![
                AsmOperand::Immediate(12),
                AsmOperand::Immediate(2),
                AsmOperand::Label("done".to_string()),
            ],
            source_line: 1,
        };
        assert_eq!(
            assemble_line(&line, &labels, 3).unwrap().unwrap(),
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            }
        );
    }
}
