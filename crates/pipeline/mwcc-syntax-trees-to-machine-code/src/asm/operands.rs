//! Operand extraction for the inline-`asm` assembler.
//!
//! Each helper reads a fixed operand shape from a parsed line (register lists,
//! `<disp>(rN)` memory forms, condition fields, immediates, and `sym@suffix`
//! relocations), returning a [`Diagnostic`] error — which DEFERS the translation
//! unit — for any unsupported form. [`super::encode::assemble_line`] calls them.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::AsmOperand;

/// Read exactly `N` GPR operands.
pub(super) fn gprs<const N: usize>(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<[u8; N]> {
    expect_operand_count(mnemonic, operands, N)?;
    let mut registers = [0u8; N];
    for (slot, operand) in registers.iter_mut().zip(operands) {
        *slot = gpr(mnemonic, operand)?;
    }
    Ok(registers)
}

/// Read exactly three GPR operands positionally (`op dst, srcA, srcB`).
pub(super) fn rrr(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<[u8; 3]> {
    gprs(mnemonic, operands)
}

/// Read exactly `N` FPR operands positionally.
pub(super) fn fprs<const N: usize>(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<[u8; N]> {
    expect_operand_count(mnemonic, operands, N)?;
    let mut registers = [0u8; N];
    for (slot, operand) in registers.iter_mut().zip(operands) {
        *slot = fpr(mnemonic, operand)?;
    }
    Ok(registers)
}

/// Split off an optional leading `crN` condition-field operand (mwcc spells
/// compares/branches with an explicit condition register). Returns the field
/// number (0 when absent) and the remaining operands.
pub(super) fn take_cr_field(operands: &[AsmOperand]) -> (u8, &[AsmOperand]) {
    match operands.first() {
        Some(AsmOperand::ConditionRegister(field)) => (*field, &operands[1..]),
        _ => (0, operands),
    }
}

/// Consume the OPTIONAL `L` (compare-length) operand of the classic `cmpi`/`cmpli`
/// spelling. It is present only in the four-operand form (`cmp{i,li} crf, L, rA, imm`
/// — three operands after the condition field, the first an immediate); the
/// three-operand form omits it. When present, L must be 0 (a 32-bit compare).
pub(super) fn strip_length_bit<'a>(
    mnemonic: &str,
    (crf, operands): (u8, &'a [AsmOperand]),
) -> Compilation<(u8, &'a [AsmOperand])> {
    match operands {
        [AsmOperand::Immediate(0), _, _] => Ok((crf, &operands[1..])),
        [AsmOperand::Immediate(_), _, _] => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' with a 64-bit length bit is not supported yet (roadmap)"
        ))),
        _ => Ok((crf, operands)),
    }
}

/// Require the condition field to be `cr0` (for instructions whose non-cr0 form
/// is not modeled yet); returns the remaining operands.
pub(super) fn require_cr0<'a>(
    mnemonic: &str,
    operands: &'a [AsmOperand],
) -> Compilation<&'a [AsmOperand]> {
    let (field, rest) = take_cr_field(operands);
    if field != 0 {
        return Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' on cr{field} (non-cr0) is not supported yet (roadmap)"
        )));
    }
    Ok(rest)
}

/// Read a `rlwinm`-family `(rA, rS, SH, MB, ME)` operand list.
pub(super) fn rotate5(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<(u8, u8, u8, u8, u8)> {
    expect_operand_count(mnemonic, operands, 5)?;
    let a = gpr(mnemonic, &operands[0])?;
    let s = gpr(mnemonic, &operands[1])?;
    let field = |operand: &AsmOperand, what: &str| -> Compilation<u8> {
        match operand {
            AsmOperand::Immediate(value) if (0..=31).contains(value) => Ok(*value as u8),
            _ => Err(Diagnostic::error(format!(
                "inline-asm '{mnemonic}' {what} must be 0..=31"
            ))),
        }
    };
    let shift = field(&operands[2], "shift")?;
    let begin = field(&operands[3], "mask begin")?;
    let end = field(&operands[4], "mask end")?;
    Ok((a, s, shift, begin, end))
}

/// Read `(GPR, GPR, imm, imm)` (`op dst, src, N, B`) — the two immediates of an
/// `extlwi`/`extrwi` rotate spelling, each 0..=31.
pub(super) fn rr_two_immediates(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<(u8, u8, u8, u8)> {
    expect_operand_count(mnemonic, operands, 4)?;
    let a = gpr(mnemonic, &operands[0])?;
    let s = gpr(mnemonic, &operands[1])?;
    let field = |operand: &AsmOperand| match operand {
        AsmOperand::Immediate(value) if (0..=31).contains(value) => Ok(*value as u8),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected an immediate in 0..=31"
        ))),
    };
    Ok((a, s, field(&operands[2])?, field(&operands[3])?))
}

/// Read a `(GPR, GPR, shift-amount)` triple (`op dst, src, SH`) where the shift is
/// a 0..=31 immediate encoded in the instruction's SH field.
pub(super) fn rr_shift(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, u8, u8)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let a = gpr(mnemonic, &operands[0])?;
    let s = gpr(mnemonic, &operands[1])?;
    let shift = match &operands[2] {
        AsmOperand::Immediate(value) if (0..=31).contains(value) => *value as u8,
        _ => {
            return Err(Diagnostic::error(format!(
                "inline-asm '{mnemonic}' expected a shift amount in 0..=31"
            )))
        }
    };
    Ok((a, s, shift))
}

/// Read a `(GPR, GPR, signed-immediate)` triple (`op dst, src, SIMM`).
pub(super) fn rri(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, u8, i16)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let d = gpr(mnemonic, &operands[0])?;
    let a = gpr(mnemonic, &operands[1])?;
    let immediate = immediate16(mnemonic, &operands[2])?;
    Ok((d, a, immediate))
}

/// Read a `(GPR, GPR, signed-immediate-or-relocated-symbol)` triple. This is
/// separate from [`rri`] because only instructions whose immediate is patched
/// directly by the linker can safely accept a relocation; aliases such as
/// `subic` transform the immediate and must remain numeric-only.
pub(super) fn rri_symbolic(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<(u8, u8, i16)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let d = gpr(mnemonic, &operands[0])?;
    let a = gpr(mnemonic, &operands[1])?;
    let immediate = signed_immediate_or_symbol(mnemonic, &operands[2])?;
    Ok((d, a, immediate))
}

/// Read a `(GPR, GPR, unsigned-immediate)` triple (`op dst, src, UIMM`).
pub(super) fn rri_u(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, u8, u16)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let a = gpr(mnemonic, &operands[0])?;
    let s = gpr(mnemonic, &operands[1])?;
    let immediate = immediate16u(mnemonic, &operands[2])?;
    Ok((a, s, immediate))
}

/// Read a `(GPR, displacement, base-GPR)` triple from `rT, <disp>(rA)`.
pub(super) fn gpr_mem(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16, u8)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = gpr(mnemonic, &operands[0])?;
    let (displacement, base) = memory(mnemonic, &operands[1])?;
    Ok((register, displacement, base))
}

/// Read a `(FPR, displacement, base-GPR)` triple from `fT, <disp>(rA)`.
pub(super) fn fpr_mem(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16, u8)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = fpr(mnemonic, &operands[0])?;
    let (displacement, base) = memory(mnemonic, &operands[1])?;
    Ok((register, displacement, base))
}

/// Read the Gekko quantized-memory form `fT, disp(rA), W, I`.
/// The displacement is a signed 12-bit field, W is one bit, and I selects one
/// of eight graphics quantization registers.
pub(super) fn quantized_fpr_mem(
    mnemonic: &str,
    operands: &[AsmOperand],
) -> Compilation<(u8, i16, u8, u8, u8)> {
    expect_operand_count(mnemonic, operands, 4)?;
    let register = fpr(mnemonic, &operands[0])?;
    let (displacement, base) = memory(mnemonic, &operands[1])?;
    if !(-2048..=2047).contains(&displacement) {
        return Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' displacement {displacement} does not fit in 12 bits"
        )));
    }
    let w = match &operands[2] {
        AsmOperand::Immediate(value @ 0..=1) => *value as u8,
        _ => {
            return Err(Diagnostic::error(format!(
                "inline-asm '{mnemonic}' W field must be 0 or 1"
            )))
        }
    };
    let i = match &operands[3] {
        AsmOperand::Immediate(value @ 0..=7) => *value as u8,
        _ => {
            return Err(Diagnostic::error(format!(
                "inline-asm '{mnemonic}' I field must be 0..=7"
            )))
        }
    };
    Ok((register, displacement, base, w, i))
}

/// Read a `(GPR, immediate)` operand pair.
pub(super) fn gpr_immediate(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = gpr(mnemonic, &operands[0])?;
    let immediate = immediate16(mnemonic, &operands[1])?;
    Ok((register, immediate))
}

fn fpr(mnemonic: &str, operand: &AsmOperand) -> Compilation<u8> {
    match operand {
        AsmOperand::Fpr(index) => Ok(*index),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected a floating-point register operand"
        ))),
    }
}

fn memory(mnemonic: &str, operand: &AsmOperand) -> Compilation<(i16, u8)> {
    match operand {
        AsmOperand::Memory { displacement, base } => Ok((*displacement, *base)),
        AsmOperand::SymbolMemory { base, .. }
        | AsmOperand::SmallDataSymbolMemory { base, .. } => Ok((0, *base)),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected a `<disp>(<reg>)` memory operand"
        ))),
    }
}

pub(super) fn immediate16u(mnemonic: &str, operand: &AsmOperand) -> Compilation<u16> {
    match operand {
        // A logical immediate is 16-bit unsigned; accept the sign-agnostic bit
        // pattern (`ori r0, r0, 0x8000` and `-0x8000` both name the same halfword).
        AsmOperand::Immediate(value) => u16::try_from(*value)
            .or_else(|_| i16::try_from(*value).map(|signed| signed as u16))
            .map_err(|_| {
                Diagnostic::error(format!(
                    "inline-asm '{mnemonic}' immediate {value} does not fit in 16 bits"
                ))
            }),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected an immediate operand"
        ))),
    }
}

pub(super) fn gpr(mnemonic: &str, operand: &AsmOperand) -> Compilation<u8> {
    match operand {
        AsmOperand::Gpr(index) => Ok(*index),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected a general-purpose register operand"
        ))),
    }
}

/// A signed 16-bit immediate, or 0 for a `sym@suffix` operand (the linker patches
/// the field from a recorded relocation).
pub(super) fn signed_immediate_or_symbol(mnemonic: &str, operand: &AsmOperand) -> Compilation<i16> {
    match operand {
        AsmOperand::Symbol { .. } | AsmOperand::SymbolMemory { .. } => Ok(0),
        _ => immediate16(mnemonic, operand),
    }
}

/// An unsigned 16-bit immediate, or 0 for a `sym@suffix` operand.
pub(super) fn unsigned_immediate_or_symbol(
    mnemonic: &str,
    operand: &AsmOperand,
) -> Compilation<u16> {
    match operand {
        AsmOperand::Symbol { .. } | AsmOperand::SymbolMemory { .. } => Ok(0),
        _ => immediate16u(mnemonic, operand),
    }
}

fn immediate16(mnemonic: &str, operand: &AsmOperand) -> Compilation<i16> {
    match operand {
        // A 16-bit immediate field: accept either the signed range or the unsigned
        // bit pattern (`lis r3, 0x8000` is written as 32768 but the field is 0x8000),
        // taking the low 16 bits either way.
        AsmOperand::Immediate(value) if (-0x8000..=0xffff).contains(value) => {
            Ok(*value as u16 as i16)
        }
        AsmOperand::Immediate(value) => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' immediate {value} does not fit in 16 bits"
        ))),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected an immediate operand"
        ))),
    }
}

pub(super) fn expect_operand_count(
    mnemonic: &str,
    operands: &[AsmOperand],
    expected: usize,
) -> Compilation<()> {
    if operands.len() != expected {
        return Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected {expected} operand(s), found {}",
            operands.len()
        )));
    }
    Ok(())
}

/// A special-purpose-register operand for `mfspr`/`mtspr`: an immediate SPR
/// number, or a Gekko SPR NAME (GQR0-7, HID0-2, L2CR, WPAR, the DMA pair). The
/// returned value is the raw SPR number; the split-field encoding is applied at
/// encode time. Names come through as a `Label` (a bare identifier the assembler
/// resolves) — mwcc's asm knows the Gekko SPR mnemonics.
pub(super) fn special_register(mnemonic: &str, operand: &AsmOperand) -> Compilation<u16> {
    let named = |name: &str| -> Option<u16> {
        Some(match name {
            "GQR0" => 912,
            "GQR1" => 913,
            "GQR2" => 914,
            "GQR3" => 915,
            "GQR4" => 916,
            "GQR5" => 917,
            "GQR6" => 918,
            "GQR7" => 919,
            // mwcc's Gekko assembler accepts DBSR as the debug-oriented alias
            // for HID0 (observed in the Dolphin system-call vector).
            "DBSR" => 1008,
            "HID0" => 1008,
            "HID1" => 1009,
            "HID2" => 920,
            "L2CR" => 1017,
            "WPAR" => 921,
            "DMAU" => 922,
            "DMAL" => 923,
            "MMCR0" => 952,
            "MMCR1" => 956,
            "PMC1" => 953,
            "PMC2" => 954,
            "PMC3" => 957,
            "PMC4" => 958,
            "DEC" => 22,
            "SDR1" => 25,
            "SPRG0" => 272,
            "SPRG1" => 273,
            "SPRG2" => 274,
            "SPRG3" => 275,
            "SRR0" => 26,
            "SRR1" => 27,
            "TBL" => 268,
            "TBU" => 269,
            "DABR" => 1013,
            "IABR" => 1010,
            "PVR" => 287,
            _ => return None,
        })
    };
    match operand {
        AsmOperand::Immediate(value) if (0..=1023).contains(value) => Ok(*value as u16),
        AsmOperand::Label(name) => named(name).ok_or_else(|| {
            Diagnostic::error(format!(
                "inline-asm '{mnemonic}' unknown special register '{name}'"
            ))
        }),
        AsmOperand::Symbol { name, .. } => named(name).ok_or_else(|| {
            Diagnostic::error(format!(
                "inline-asm '{mnemonic}' unknown special register '{name}'"
            ))
        }),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected an SPR number or name"
        ))),
    }
}

/// A general-purpose register, or the literal `0` written as an immediate (a
/// cache op's `dcbt 0, rB` base — an rA=0 encoding, not a use of r0).
pub(super) fn gpr_or_zero(mnemonic: &str, operand: &AsmOperand) -> Compilation<u8> {
    match operand {
        AsmOperand::Gpr(index) => Ok(*index),
        AsmOperand::Immediate(0) => Ok(0),
        _ => Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected a register or 0 base"
        ))),
    }
}
