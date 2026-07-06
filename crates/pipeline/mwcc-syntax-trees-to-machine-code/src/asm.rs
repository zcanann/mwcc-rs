//! Inline-`asm` function assembly.
//!
//! A Metrowerks `asm` function body is emitted VERBATIM — mwcc assembles the
//! written instructions with no register allocation, scheduling, or optimizer
//! pass, appending a trailing `blr` when the body does not already end in a
//! branch/return. This module turns the parsed [`AsmInstruction`] lines into the
//! shared [`Instruction`] stream (which the object writer already encodes), so
//! the ordinary codegen path is bypassed entirely for these functions.
//!
//! The supported mnemonic set is deliberately small and grows one verified
//! shape at a time (each backed by an oracle canary). An unsupported mnemonic or
//! operand form is an ERROR, so its translation unit DEFERS rather than risking
//! wrong bytes — the byte-exact-or-defer invariant.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{AsmInstruction, AsmOperand, Function};

/// Assemble an inline-`asm` function into a finished [`MachineFunction`]. The
/// caller has already established `function.asm_body` is `Some`.
pub(crate) fn assemble_asm_function(function: &Function) -> Compilation<MachineFunction> {
    let body = function
        .asm_body
        .as_ref()
        .expect("assemble_asm_function called on a non-asm function");

    let mut instructions = Vec::new();
    for line in body {
        if let Some(instruction) = assemble_line(line)? {
            instructions.push(instruction);
        }
    }
    // mwcc appends an implicit `blr` unless the body already ends in a control
    // transfer (an explicit `blr`, an unconditional branch, …).
    if !instructions.last().is_some_and(is_terminator) {
        instructions.push(Instruction::BranchToLinkRegister);
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = instructions;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.is_asm = true;
    Ok(output)
}

/// Whether an instruction ends control flow (so no implicit `blr` is appended).
fn is_terminator(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::BranchToLinkRegister
            | Instruction::Branch { .. }
            | Instruction::BranchConditionalToLinkRegister { .. }
    )
}

/// Assemble one asm line into an instruction, or `None` for a directive that
/// emits nothing (`nofralloc`).
fn assemble_line(line: &AsmInstruction) -> Compilation<Option<Instruction>> {
    let mnemonic = line.mnemonic.as_str();
    let operands = &line.operands;
    let instruction = match mnemonic {
        // `nofralloc` suppresses the auto-generated stack frame. For the register-
        // only bodies supported so far no frame is generated regardless, so it is a
        // no-op directive; it emits nothing.
        "nofralloc" => return Ok(None),
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
        // `blr` — return (branch to link register).
        "blr" => {
            expect_operand_count(mnemonic, operands, 0)?;
            Instruction::BranchToLinkRegister
        }
        other => return Err(Diagnostic::error(format!("inline-asm mnemonic '{other}' is not supported yet (roadmap)"))),
    };
    Ok(Some(instruction))
}

/// Read exactly `N` GPR operands.
fn gprs<const N: usize>(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<[u8; N]> {
    expect_operand_count(mnemonic, operands, N)?;
    let mut registers = [0u8; N];
    for (slot, operand) in registers.iter_mut().zip(operands) {
        *slot = gpr(mnemonic, operand)?;
    }
    Ok(registers)
}

/// Read a `(GPR, immediate)` operand pair.
fn gpr_immediate(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = gpr(mnemonic, &operands[0])?;
    let immediate = immediate16(mnemonic, &operands[1])?;
    Ok((register, immediate))
}

fn gpr(mnemonic: &str, operand: &AsmOperand) -> Compilation<u8> {
    match operand {
        AsmOperand::Gpr(index) => Ok(*index),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected a general-purpose register operand"))),
    }
}

fn immediate16(mnemonic: &str, operand: &AsmOperand) -> Compilation<i16> {
    match operand {
        AsmOperand::Immediate(value) => i16::try_from(*value)
            .map_err(|_| Diagnostic::error(format!("inline-asm '{mnemonic}' immediate {value} does not fit in 16 bits"))),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected an immediate operand"))),
    }
}

fn expect_operand_count(mnemonic: &str, operands: &[AsmOperand], expected: usize) -> Compilation<()> {
    if operands.len() != expected {
        return Err(Diagnostic::error(format!(
            "inline-asm '{mnemonic}' expected {expected} operand(s), found {}",
            operands.len()
        )));
    }
    Ok(())
}
