//! After coloring: EABI frame, prologue/epilogue, final scheduling, and the
//! physical [`MachineFunction`].

use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, MachineFunction, Relocation};
use mwcc_pcode::PCodeFunction;

use crate::coloring;

/// Scheduling policy for the final code.
#[derive(Debug, Clone, Copy)]
pub struct FinishOptions {
    /// `-O4`-style latency scheduling of the physical code.
    pub schedule: bool,
    /// Remove dead definitions while building interference (MWCC's
    /// `gDeleteDeadInstructions`).
    pub delete_dead: bool,
}

/// Color `pcode` and produce its final machine function.
pub fn finish(
    mut pcode: PCodeFunction,
    makes_calls: bool,
    options: FinishOptions,
) -> Compilation<MachineFunction> {
    let colors = coloring::color(&mut pcode, options.delete_dead)?;
    if !colors.saved_float.is_empty() {
        return Err(mwcc_core::Diagnostic::error(
            "PCode frame: saved float registers are not yet supported",
        ));
    }
    let framed = makes_calls || !colors.saved_general.is_empty();
    let plan = mwcc_vreg::FramePlan::sized_for(colors.saved_general.clone());

    let mut instructions: Vec<Instruction> = Vec::new();
    let mut relocations: Vec<Relocation> = Vec::new();
    if framed {
        instructions.extend(plan.prologue());
    }
    let body_blocks = pcode.blocks.len().saturating_sub(1);
    for block in &pcode.blocks[..body_blocks] {
        for instruction in &block.instructions {
            if let Some(relocation) = &instruction.relocation {
                relocations.push(Relocation {
                    instruction_index: instructions.len(),
                    kind: relocation.kind,
                    target: relocation.target.clone(),
                });
            }
            instructions.push(instruction.instruction.clone());
        }
    }
    if framed {
        instructions.extend(plan.epilogue());
    } else {
        instructions.push(Instruction::BranchToLinkRegister);
    }

    if options.schedule {
        let permutation = mwcc_vreg::schedule(&mut instructions);
        remap(&mut relocations, &permutation);
        let permutation = mwcc_vreg::schedule_link_register_save(&mut instructions);
        remap(&mut relocations, &permutation);
        let permutation = mwcc_vreg::hoist_link_register_reload(&mut instructions, &[], true);
        remap(&mut relocations, &permutation);
    }

    let mut output = MachineFunction::new(pcode.name.clone());
    output.instructions = instructions;
    output.relocations = relocations;
    Ok(output)
}

fn remap(relocations: &mut [Relocation], permutation: &[usize]) {
    for relocation in relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
}
