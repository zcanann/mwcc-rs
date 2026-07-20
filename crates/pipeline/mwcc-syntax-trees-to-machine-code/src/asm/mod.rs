//! Inline-`asm` function assembly.
//!
//! A Metrowerks `asm` function body is assembled without the ordinary C/C++
//! register allocator, scheduler, or optimizer. The written instructions remain
//! verbatim, while assembler directives can request a generated linkage frame
//! and the compiler may append a trailing `blr`. This module turns parsed
//! [`AsmInstruction`] lines into the shared [`Instruction`] stream, so ordinary
//! function-body codegen is bypassed entirely.
//!
//! The supported mnemonic set is deliberately small and grows one verified
//! shape at a time (each backed by an oracle canary). An unsupported mnemonic or
//! operand form is an ERROR, so its translation unit DEFERS rather than risking
//! wrong bytes — the byte-exact-or-defer invariant.
//!
//! The assembler is split by responsibility: this driver owns two-pass label and
//! relocation resolution plus branch peepholes; [`encode`] matches mnemonics;
//! [`operands`] extracts operands; and [`frame`] synthesizes linkage frames.

mod encode;
mod frame;
mod operands;

use encode::assemble_line;
use frame::{wrap_auto_frame, wrap_fralloc_frame};

use mwcc_core::Compilation;
use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{AsmInstruction, AsmItem, AsmOperand, AsmRelocSuffix, Function};
use mwcc_versions::{AsmBranchOptimizationStyle, AsmFunctionFinalizationStyle, Behavior};
use std::collections::HashMap;

/// Assemble an inline-`asm` function into a finished [`MachineFunction`]. The
/// caller has already established `function.asm_body` is `Some`.
pub(crate) fn assemble_asm_function(
    function: &Function,
    behavior: Behavior,
) -> Compilation<MachineFunction> {
    let body = function
        .asm_body
        .as_ref()
        .expect("assemble_asm_function called on a non-asm function");

    // In 2.4.x, an asm function WITHOUT `nofralloc` that uses the stack gets a
    // generated 16-byte frame around its body (BfBB's runtime helpers). Build
    // 163 leaves the written frame verbatim; a frameless leaf stays verbatim in
    // either generation.
    let mnemonics = |name: &str| {
        body.iter()
            .any(|item| matches!(item, AsmItem::Instruction(line) if line.mnemonic == name))
    };
    let auto_frame = behavior.asm_function_finalization_style
        == AsmFunctionFinalizationStyle::GeneratedFrame
        && !mnemonics("nofralloc")
        && !mnemonics("fralloc")
        && mnemonics("stwu");
    let fralloc_frame = mnemonics("fralloc")
        && body.iter().any(|item| {
            matches!(
                item,
                AsmItem::Instruction(line)
                    if matches!(line.mnemonic.as_str(), "bl" | "blrl" | "bctrl")
            )
        });

    // Pass 1: map each label to the index of the instruction it precedes (a label
    // with no following instruction points one past the end — the auto-`blr` slot),
    // and record each `entry <name>` at the same instruction position.
    let mut labels: HashMap<&str, usize> = HashMap::new();
    let mut entry_points: Vec<(String, usize)> = Vec::new();
    let mut index = 0usize;
    for item in body {
        match item {
            AsmItem::Label(name) => {
                labels.insert(name.as_str(), index);
            }
            AsmItem::Entry(name) => {
                entry_points.push((name.clone(), index));
            }
            AsmItem::Instruction(line) if emits_word(line) => index += 1,
            AsmItem::Instruction(_) => {}
        }
    }

    // Pass 2: assemble each instruction, resolving branch targets from the label map
    // and recording a relocation for any `sym@suffix` operand (against the symbol,
    // patched by the linker).
    let mut instructions = Vec::new();
    let mut relocations: Vec<Relocation> = Vec::new();
    let mut symbol_order: Vec<String> = Vec::new();
    // For an auto-frame function, the `frfree` directive marks where mwcc inserts the
    // frame-teardown epilogue (BfBB's `…; addi; frfree; blr`).
    let mut frfree_position: Option<usize> = None;
    for item in body {
        if let AsmItem::Instruction(line) = item {
            if line.mnemonic == "frfree" {
                frfree_position = Some(instructions.len());
            }
            let instruction_index = instructions.len();
            if let Some(instruction) = assemble_line(line, &labels, instruction_index)? {
                instructions.push(instruction);
                for operand in &line.operands {
                    if let AsmOperand::Symbol { name, suffix }
                    | AsmOperand::SymbolMemory { name, suffix, .. } = operand
                    {
                        relocations.push(Relocation {
                            instruction_index,
                            kind: relocation_kind(*suffix),
                            target: RelocationTarget::External(name.clone()),
                        });
                        if !symbol_order.contains(name) {
                            symbol_order.push(name.clone());
                        }
                    }
                }
                // A `b func` / `bl func` whose target is not a local label is a tail branch or
                // call to an external function: record its `R_PPC_REL24` relocation (the word
                // itself is assembled as an offset-0 placeholder).
                if matches!(line.mnemonic.as_str(), "b" | "bl") {
                    if let Some(AsmOperand::Label(name)) = line.operands.first() {
                        if !labels.contains_key(name.as_str()) {
                            relocations.push(Relocation {
                                instruction_index,
                                kind: RelocationKind::Rel24,
                                target: RelocationTarget::External(name.clone()),
                            });
                            if !symbol_order.contains(name) {
                                symbol_order.push(name.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    // Mainline appends `blr` only for fallthrough; build 163 appends one after
    // every written body, including one that already ends in a control transfer.
    // `nofralloc` suppresses both policies and remains fully verbatim (measured:
    // OSSync.c's SystemCallVector ends `rfi; entry …End; nop`).
    let written_body_end = instructions.len();
    let append_terminal_return = !mnemonics("nofralloc")
        && match behavior.asm_function_finalization_style {
            AsmFunctionFinalizationStyle::GeneratedFrame => {
                !instructions.last().is_some_and(is_terminator)
            }
            AsmFunctionFinalizationStyle::VerbatimFrameWithTerminalReturn => true,
        };
    if append_terminal_return {
        instructions.push(Instruction::BranchToLinkRegister);
    }
    // mwcc's asm branch peepholes (both discovered by probe): a branch whose target
    // is another unconditional branch chases to the final target; a branch whose
    // final target is a `blr` becomes the branch-to-link form.
    if behavior.asm_branch_optimization_style == AsmBranchOptimizationStyle::ChaseAndCollapseReturns
    {
        apply_branch_peepholes(&mut instructions);
    }
    if fralloc_frame {
        wrap_fralloc_frame(
            &mut instructions,
            &mut relocations,
            &mut entry_points,
            frfree_position.unwrap_or(written_body_end),
            behavior.frame_convention,
            behavior.plain_linkage_epilogue_style,
        )?;
    } else if auto_frame {
        wrap_auto_frame(
            &mut instructions,
            &mut relocations,
            &mut entry_points,
            frfree_position.unwrap_or(written_body_end),
        )?;
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = instructions;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.is_asm = true;
    output.entry_points = entry_points;
    output.force_active = function.force_active;
    output.relocations = relocations;
    output.symbol_order = symbol_order;
    Ok(output)
}

/// Whether an assembled line contributes a machine word (the register-allocation
/// directives do not) — used to number instructions for label resolution.
fn emits_word(line: &AsmInstruction) -> bool {
    !matches!(line.mnemonic.as_str(), "fralloc" | "nofralloc" | "frfree")
}

/// Reproduce mwcc's two inline-asm branch peepholes, preserving instruction indices:
///  1. CHAIN: a branch whose target is an unconditional `b` is retargeted to that
///     branch's destination (followed transitively).
///  2. RETURN: a branch whose (chased) target is a `blr` becomes the branch-to-link
///     form (`b <ret>` -> `blr`, `blt <ret>` -> `bltlr`).
fn apply_branch_peepholes(instructions: &mut [Instruction]) {
    let count = instructions.len();
    // Snapshot: unconditional-branch destinations and return positions.
    let unconditional: Vec<Option<usize>> = instructions
        .iter()
        .map(|instruction| match instruction {
            Instruction::Branch { target } => Some(*target),
            _ => None,
        })
        .collect();
    let is_return: Vec<bool> = instructions
        .iter()
        .map(|instruction| matches!(instruction, Instruction::BranchToLinkRegister))
        .collect();
    // Follow a chain of unconditional branches to its final landing index.
    let chase = |mut target: usize| -> usize {
        let mut steps = 0;
        while let Some(Some(next)) = unconditional.get(target).copied() {
            target = next;
            steps += 1;
            if steps > count {
                break; // guard against a pathological branch cycle
            }
        }
        target
    };
    for instruction in instructions.iter_mut() {
        match *instruction {
            Instruction::Branch { target } => {
                let landing = chase(target);
                if is_return.get(landing).copied().unwrap_or(false) {
                    *instruction = Instruction::BranchToLinkRegister;
                } else {
                    *instruction = Instruction::Branch { target: landing };
                }
            }
            Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target,
            } => {
                let landing = chase(target);
                if is_return.get(landing).copied().unwrap_or(false) {
                    *instruction = Instruction::BranchConditionalToLinkRegister {
                        options,
                        condition_bit,
                    };
                } else {
                    *instruction = Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: landing,
                    };
                }
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
            // `mtctr r12; bctr` — the ptmf tail dispatch ends the function.
            | Instruction::BranchToCountRegister
    )
}

/// The relocation kind for a `@`-suffix on an asm symbol operand.
fn relocation_kind(suffix: AsmRelocSuffix) -> RelocationKind {
    match suffix {
        AsmRelocSuffix::Hi => RelocationKind::Addr16Hi,
        AsmRelocSuffix::Ha => RelocationKind::Addr16Ha,
        AsmRelocSuffix::Lo => RelocationKind::Addr16Lo,
    }
}
