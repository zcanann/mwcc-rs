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
use mwcc_syntax_trees::{AsmInstruction, AsmItem, AsmOperand, Function};
use std::collections::HashMap;

/// Assemble an inline-`asm` function into a finished [`MachineFunction`]. The
/// caller has already established `function.asm_body` is `Some`.
pub(crate) fn assemble_asm_function(function: &Function) -> Compilation<MachineFunction> {
    let body = function
        .asm_body
        .as_ref()
        .expect("assemble_asm_function called on a non-asm function");

    // Pass 1: map each label to the index of the instruction it precedes (a label
    // with no following instruction points one past the end — the auto-`blr` slot).
    let mut labels: HashMap<&str, usize> = HashMap::new();
    let mut index = 0usize;
    for item in body {
        match item {
            AsmItem::Label(name) => {
                labels.insert(name.as_str(), index);
            }
            AsmItem::Instruction(line) if emits_word(line) => index += 1,
            AsmItem::Instruction(_) => {}
        }
    }

    // Pass 2: assemble each instruction, resolving branch targets from the label map.
    let mut instructions = Vec::new();
    for item in body {
        if let AsmItem::Instruction(line) = item {
            if let Some(instruction) = assemble_line(line, &labels)? {
                instructions.push(instruction);
            }
        }
    }
    // mwcc appends an implicit `blr` unless the body already ends in a control
    // transfer (an explicit `blr`, an unconditional branch, …).
    if !instructions.last().is_some_and(is_terminator) {
        instructions.push(Instruction::BranchToLinkRegister);
    }
    // mwcc's asm peephole: a branch whose target is a `blr` becomes the
    // corresponding branch-to-link form (`b <ret>` -> `blr`, `blt <ret>` -> `bltlr`).
    apply_branch_to_return_peephole(&mut instructions);

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = instructions;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.is_asm = true;
    Ok(output)
}

/// Whether an assembled line contributes a machine word (a directive like
/// `nofralloc` does not) — used to number instructions for label resolution.
fn emits_word(line: &AsmInstruction) -> bool {
    line.mnemonic != "nofralloc"
}

/// Rewrite each branch whose target instruction is a `blr` into the branch-to-link
/// form, reproducing mwcc's inline-asm peephole. Indices are preserved (1-for-1).
fn apply_branch_to_return_peephole(instructions: &mut [Instruction]) {
    let is_return: Vec<bool> = instructions
        .iter()
        .map(|instruction| matches!(instruction, Instruction::BranchToLinkRegister))
        .collect();
    for instruction in instructions.iter_mut() {
        match *instruction {
            Instruction::Branch { target } if is_return.get(target).copied().unwrap_or(false) => {
                *instruction = Instruction::BranchToLinkRegister;
            }
            Instruction::BranchConditionalForward { options, condition_bit, target }
                if is_return.get(target).copied().unwrap_or(false) =>
            {
                *instruction = Instruction::BranchConditionalToLinkRegister { options, condition_bit };
            }
            _ => {}
        }
    }
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
/// emits nothing (`nofralloc`). Branch mnemonics resolve their target label
/// through `labels` (label name -> instruction index).
fn assemble_line(line: &AsmInstruction, labels: &HashMap<&str, usize>) -> Compilation<Option<Instruction>> {
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
        // `lis rD, SIMM` — load immediate shifted (`addis rD, r0, SIMM`).
        "lis" => {
            let (d, immediate) = gpr_immediate(mnemonic, operands)?;
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

        // Register + signed-immediate ALU (`op dst, src, SIMM`).
        "addi" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediate { d, a, immediate } }
        "addis" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::AddImmediateShifted { d, a, immediate } }
        "subfic" => { let (d, a, immediate) = rri(mnemonic, operands)?; Instruction::SubtractFromImmediate { d, a, immediate } }
        // Register + unsigned-immediate logical (`op dst, src, UIMM`).
        "ori" => { let (a, s, immediate) = rri_u(mnemonic, operands)?; Instruction::OrImmediate { a, s, immediate } }
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

        // Compares against cr0 (`cmp rA, {rB | SIMM}`). An explicit `crN` field
        // (a 3-operand form) is not modeled and reaches the DEFER fallthrough.
        "cmpwi" => { let (a, immediate) = gpr_immediate(mnemonic, operands)?; Instruction::CompareWordImmediate { a, immediate } }
        "cmplwi" => {
            expect_operand_count(mnemonic, operands, 2)?;
            let a = gpr(mnemonic, &operands[0])?;
            let immediate = immediate16u(mnemonic, &operands[1])?;
            Instruction::CompareLogicalWordImmediate { a, immediate }
        }
        "cmpw" => { let [a, b] = gprs(mnemonic, operands)?; Instruction::CompareWord { a, b } }
        "cmplw" => { let [a, b] = gprs(mnemonic, operands)?; Instruction::CompareLogicalWord { a, b } }

        // The count register (`bdnz` loop support).
        "mtctr" => { let [s] = gprs(mnemonic, operands)?; Instruction::MoveToCountRegister { s } }

        // Unconditional branch to a label.
        "b" => Instruction::Branch { target: label_target(mnemonic, operands, labels)? },
        // Conditional branches on cr0 (BO/BI): the target is a label.
        "beq" | "bne" | "blt" | "bge" | "bgt" | "ble" | "bdnz" => {
            let (options, condition_bit) = conditional_branch_fields(mnemonic);
            let target = label_target(mnemonic, operands, labels)?;
            Instruction::BranchConditionalForward { options, condition_bit, target }
        }

        other => return Err(Diagnostic::error(format!("inline-asm mnemonic '{other}' is not supported yet (roadmap)"))),
    };
    Ok(Some(instruction))
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

/// Read exactly `N` GPR operands.
fn gprs<const N: usize>(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<[u8; N]> {
    expect_operand_count(mnemonic, operands, N)?;
    let mut registers = [0u8; N];
    for (slot, operand) in registers.iter_mut().zip(operands) {
        *slot = gpr(mnemonic, operand)?;
    }
    Ok(registers)
}

/// Read exactly three GPR operands positionally (`op dst, srcA, srcB`).
fn rrr(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<[u8; 3]> {
    gprs(mnemonic, operands)
}

/// Read a `(GPR, GPR, signed-immediate)` triple (`op dst, src, SIMM`).
fn rri(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, u8, i16)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let d = gpr(mnemonic, &operands[0])?;
    let a = gpr(mnemonic, &operands[1])?;
    let immediate = immediate16(mnemonic, &operands[2])?;
    Ok((d, a, immediate))
}

/// Read a `(GPR, GPR, unsigned-immediate)` triple (`op dst, src, UIMM`).
fn rri_u(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, u8, u16)> {
    expect_operand_count(mnemonic, operands, 3)?;
    let a = gpr(mnemonic, &operands[0])?;
    let s = gpr(mnemonic, &operands[1])?;
    let immediate = immediate16u(mnemonic, &operands[2])?;
    Ok((a, s, immediate))
}

/// Read a `(GPR, displacement, base-GPR)` triple from `rT, <disp>(rA)`.
fn gpr_mem(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16, u8)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = gpr(mnemonic, &operands[0])?;
    let (displacement, base) = memory(mnemonic, &operands[1])?;
    Ok((register, displacement, base))
}

/// Read a `(FPR, displacement, base-GPR)` triple from `fT, <disp>(rA)`.
fn fpr_mem(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16, u8)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = fpr(mnemonic, &operands[0])?;
    let (displacement, base) = memory(mnemonic, &operands[1])?;
    Ok((register, displacement, base))
}

/// Read a `(GPR, immediate)` operand pair.
fn gpr_immediate(mnemonic: &str, operands: &[AsmOperand]) -> Compilation<(u8, i16)> {
    expect_operand_count(mnemonic, operands, 2)?;
    let register = gpr(mnemonic, &operands[0])?;
    let immediate = immediate16(mnemonic, &operands[1])?;
    Ok((register, immediate))
}

fn fpr(mnemonic: &str, operand: &AsmOperand) -> Compilation<u8> {
    match operand {
        AsmOperand::Fpr(index) => Ok(*index),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected a floating-point register operand"))),
    }
}

fn memory(mnemonic: &str, operand: &AsmOperand) -> Compilation<(i16, u8)> {
    match operand {
        AsmOperand::Memory { displacement, base } => Ok((*displacement, *base)),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected a `<disp>(<reg>)` memory operand"))),
    }
}

fn immediate16u(mnemonic: &str, operand: &AsmOperand) -> Compilation<u16> {
    match operand {
        // A logical immediate is 16-bit unsigned; accept the sign-agnostic bit
        // pattern (`ori r0, r0, 0x8000` and `-0x8000` both name the same halfword).
        AsmOperand::Immediate(value) => u16::try_from(*value)
            .or_else(|_| i16::try_from(*value).map(|signed| signed as u16))
            .map_err(|_| Diagnostic::error(format!("inline-asm '{mnemonic}' immediate {value} does not fit in 16 bits"))),
        _ => Err(Diagnostic::error(format!("inline-asm '{mnemonic}' expected an immediate operand"))),
    }
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
