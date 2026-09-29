//! After INITIAL CODE, in MWCC's `CodeGen_Generator` order: instruction
//! scheduling on virtual registers, register coloring, prologue/epilogue
//! generation, final scheduling, and the physical [`MachineFunction`].
//!
//! Branch instructions carry *block* indices as targets until flattening
//! converts them to instruction positions.

use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, MachineFunction, Relocation};
use mwcc_pcode::{Block, PCodeFunction, PInstr};

use crate::{coloring, schedule};

/// Scheduling policy for the final code.
#[derive(Debug, Clone, Copy)]
pub struct FinishOptions {
    /// MWCC's scheduler runs (`-O4`-style latency scheduling).
    pub schedule: bool,
    /// Remove dead definitions while building interference (MWCC's
    /// `gDeleteDeadInstructions`).
    pub delete_dead: bool,
}

/// Schedule, color, frame, and flatten `pcode`. The last block is the exit
/// (return) block.
pub fn finish(
    mut pcode: PCodeFunction,
    makes_calls: bool,
    options: FinishOptions,
) -> Compilation<MachineFunction> {
    prune_unreachable(&mut pcode);
    dump(&pcode, "INITIAL CODE");
    if options.schedule && !toggle("MWCC_PCODE_NO_PRESCHEDULE") {
        for block in &mut pcode.blocks {
            schedule::schedule_block(&mut block.instructions, true);
        }
    }
    dump(&pcode, "AFTER INSTRUCTION SCHEDULING");
    let colors = coloring::color(&mut pcode, options.delete_dead)?;
    dump(&pcode, "AFTER REGISTER COLORING");
    if !colors.saved_float.is_empty() {
        return Err(mwcc_core::Diagnostic::error(
            "PCode frame: saved float registers are not yet supported",
        ));
    }
    let framed = makes_calls || !colors.saved_general.is_empty();
    let plan = mwcc_vreg::FramePlan::sized_for(colors.saved_general.clone());

    let wrap = |instructions: Vec<Instruction>| -> Vec<PInstr> {
        instructions.into_iter().map(PInstr::new).collect()
    };
    let exit = pcode.blocks.len() - 1;
    let mut framed_blocks = vec![false; pcode.blocks.len()];
    if framed {
        let mut entry = wrap(plan.prologue());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        pcode.blocks[exit].instructions.extend(wrap(plan.epilogue()));
        framed_blocks[0] = true;
        framed_blocks[exit] = true;
    } else {
        pcode.blocks[exit]
            .instructions
            .push(PInstr::new(Instruction::BranchToLinkRegister));
    }
    // MWCC's "MERGING EPILOGUE, PROLOGUE": the return block joins a sole
    // fall-through predecessor, so final scheduling sees one block.
    if exit > 0 {
        let predecessors: Vec<usize> = (0..exit)
            .filter(|&block| pcode.blocks[block].successors.contains(&exit))
            .collect();
        if let [predecessor] = predecessors.as_slice() {
            if *predecessor == exit - 1 && !ends_in_branch(&pcode.blocks[exit - 1]) {
                let mut exit_block = pcode.blocks.pop().expect("exit block");
                pcode.blocks[exit - 1].instructions.append(&mut exit_block.instructions);
                pcode.blocks[exit - 1].successors.retain(|&successor| successor != exit);
                let exit_framed = framed_blocks.pop().expect("exit flag");
                framed_blocks[exit - 1] |= exit_framed;
            }
        }
    }
    // Blocks that received frame code are rescheduled; the first pass's order
    // stands elsewhere.
    if options.schedule && !toggle("MWCC_PCODE_NO_FINAL_SCHEDULE") {
        for (block, contents) in pcode.blocks.iter_mut().enumerate() {
            if framed_blocks[block] || toggle("MWCC_PCODE_FINAL_SCHEDULE") {
                schedule::schedule_block(&mut contents.instructions, false);
            }
        }
    }
    let exit_block = pcode.blocks.len() - 1;
    let frameless_exit = !framed
        && matches!(
            pcode.blocks[exit_block].instructions.as_slice(),
            [only] if matches!(only.instruction, Instruction::BranchToLinkRegister)
        );

    // Drop jumps to the next block and return inline from a frameless function.
    for block in 0..pcode.blocks.len() {
        let Some(Instruction::Branch { target }) = pcode.blocks[block]
            .instructions
            .last()
            .map(|instruction| instruction.instruction.clone())
        else {
            continue;
        };
        if target > block && (block + 1..target).all(|b| pcode.blocks[b].instructions.is_empty()) {
            pcode.blocks[block].instructions.pop();
        } else if frameless_exit && target == exit_block {
            pcode.blocks[block].instructions.last_mut().expect("branch").instruction =
                Instruction::BranchToLinkRegister;
        }
    }
    let mut starts = Vec::with_capacity(pcode.blocks.len() + 1);
    let mut position = 0;
    for block in &pcode.blocks {
        starts.push(position);
        position += block.instructions.len();
    }
    starts.push(position);

    let mut instructions: Vec<Instruction> = Vec::new();
    let mut relocations: Vec<Relocation> = Vec::new();
    for block in &pcode.blocks {
        for instruction in &block.instructions {
            if let Some(relocation) = &instruction.relocation {
                relocations.push(Relocation {
                    instruction_index: instructions.len(),
                    kind: relocation.kind,
                    target: relocation.target.clone(),
                });
            }
            let mut emitted = instruction.instruction.clone();
            match &mut emitted {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => {
                    *target = starts[*target];
                }
                _ => {}
            }
            instructions.push(emitted);
        }
    }
    let mut output = MachineFunction::new(pcode.name.clone());
    output.instructions = instructions;
    output.relocations = relocations;
    Ok(output)
}

fn ends_in_branch(block: &Block) -> bool {
    matches!(
        block.instructions.last().map(|instruction| &instruction.instruction),
        Some(Instruction::Branch { .. })
    )
}

/// Remove blocks unreachable from the entry, renumbering successors and
/// branch targets. The exit block is always kept last.
fn prune_unreachable(pcode: &mut PCodeFunction) {
    let count = pcode.blocks.len();
    let exit = count - 1;
    let mut reachable = vec![false; count];
    let mut stack = vec![0];
    while let Some(block) = stack.pop() {
        if std::mem::replace(&mut reachable[block], true) {
            continue;
        }
        stack.extend(pcode.blocks[block].successors.iter().copied());
    }
    reachable[exit] = true;
    if reachable.iter().all(|&kept| kept) {
        return;
    }
    let mut renumber = vec![usize::MAX; count];
    let mut next = 0;
    for block in 0..count {
        if reachable[block] {
            renumber[block] = next;
            next += 1;
        }
    }
    let blocks = std::mem::take(&mut pcode.blocks);
    for (index, mut block) in blocks.into_iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        block.successors = block.successors.iter().map(|&successor| renumber[successor]).collect();
        for instruction in &mut block.instructions {
            match &mut instruction.instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => {
                    *target = renumber[*target];
                }
                _ => {}
            }
        }
        pcode.blocks.push(block);
    }
}

/// `MWCC_PCODE_DUMP=<function>` prints PCode at each stage boundary, labeled
/// like MWCC's own dumps.
fn dump(pcode: &PCodeFunction, stage: &str) {
    if std::env::var("MWCC_PCODE_DUMP").is_ok_and(|name| name == pcode.name || name == "1") {
        eprintln!("== {} {stage}", pcode.name);
        for (index, block) in pcode.blocks.iter().enumerate() {
            eprintln!("  block {index} -> {:?}", block.successors);
            for instruction in &block.instructions {
                eprintln!("    {:?}", instruction.instruction);
            }
        }
    }
}

/// Model-study switches (not part of the compiler's behavior).
pub(crate) fn toggle(name: &str) -> bool {
    std::env::var_os(name).is_some()
}
