//! After INITIAL CODE, in MWCC's `CodeGen_Generator` order: instruction
//! scheduling on virtual registers, register coloring, prologue/epilogue
//! generation, final scheduling, and the physical [`MachineFunction`].

use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, MachineFunction, Relocation};
use mwcc_pcode::{PCodeFunction, PInstr};

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

/// Schedule, color, frame, and flatten `pcode`.
pub fn finish(
    mut pcode: PCodeFunction,
    makes_calls: bool,
    options: FinishOptions,
) -> Compilation<MachineFunction> {
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
    if framed {
        let mut entry = wrap(plan.prologue());
        entry.append(&mut pcode.blocks[0].instructions);
        pcode.blocks[0].instructions = entry;
        pcode.blocks[exit].instructions.extend(wrap(plan.epilogue()));
    } else {
        pcode.blocks[exit]
            .instructions
            .push(PInstr::new(Instruction::BranchToLinkRegister));
    }
    // MWCC's "MERGING EPILOGUE, PROLOGUE": the return block joins its sole
    // fall-through predecessor, so final scheduling sees one block.
    if pcode.blocks.len() == 2 && pcode.blocks[0].successors == [1] {
        let mut exit_block = pcode.blocks.pop().expect("two blocks");
        pcode.blocks[0].instructions.append(&mut exit_block.instructions);
        pcode.blocks[0].successors.clear();
    }
    // The final pass skips blocks the first pass already scheduled (flag 8);
    // body blocks keep their pre-coloring order. MWCC_PCODE_FINAL_SCHEDULE=1
    // reschedules them anyway (model study).
    // Blocks that received prologue/epilogue code are rescheduled (a leaf
    // function has none, so its first-pass order stands).
    if options.schedule && (framed || toggle("MWCC_PCODE_FINAL_SCHEDULE"))
        && !toggle("MWCC_PCODE_NO_FINAL_SCHEDULE")
    {
        for block in &mut pcode.blocks {
            schedule::schedule_block(&mut block.instructions, false);
        }
    }

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
            instructions.push(instruction.instruction.clone());
        }
    }
    let mut output = MachineFunction::new(pcode.name.clone());
    output.instructions = instructions;
    output.relocations = relocations;
    Ok(output)
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
