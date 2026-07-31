//! Late cleanup of structured control-flow forwarding blocks.

use crate::{Generator, remove_instruction_retargeting_to_next};
use mwcc_machine_code::Instruction;

/// Retarget a forward conditional through an unconditional branch at its
/// destination. The forwarding branch remains because the conditional's
/// fallthrough path may still need it; only the path that would branch to that
/// instruction can safely jump directly to its landing.
pub(crate) fn thread_conditional_branch_targets(instructions: &mut [Instruction]) {
    for index in 0..instructions.len() {
        let Instruction::BranchConditionalForward { target, .. } = instructions[index] else {
            continue;
        };
        let Some(Instruction::Branch { target: landing }) = instructions.get(target) else {
            continue;
        };
        let landing = *landing;
        if landing <= index || landing == target {
            continue;
        }
        let Instruction::BranchConditionalForward { target, .. } = &mut instructions[index] else {
            unreachable!("the conditional branch was matched above");
        };
        *target = landing;
    }
}

/// Thread incoming branches through an otherwise unreachable one-branch
/// forwarding block, then remove that dead block. A proven semantic transaction
/// may also discard a block with no incoming edge; other functions retain that
/// optimizer residue because MWCC does.
///
/// The structured statement emitter can leave this shape when one arm returns
/// and its sibling falls into an enclosing join:
///
/// ```text
/// bne forwarding
/// li result
/// b join
/// forwarding: b fallback
/// ```
///
/// MWCC retargets the conditional directly to `fallback`. Restrict this pass to
/// functions without secondary instruction-index owners until the common
/// remapper covers those owners too.
pub(crate) fn collapse_forwarding_branch_blocks(generator: &mut Generator) {
    if !generator.output.entry_points.is_empty()
        || !generator.output.jump_tables.is_empty()
        || !generator.output.data_section_displacements.is_empty()
    {
        return;
    }

    let allow_unreferenced = generator.structured_cfg_cleanup_owner;
    while let Some((index, landing)) = forwarding_branch_block(
        &generator.output.instructions,
        allow_unreferenced,
    ) {
        for instruction in &mut generator.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target == index =>
                {
                    *target = landing;
                }
                _ => {}
            }
        }
        remove_instruction_retargeting_to_next(generator, index);
    }
}

/// Remove an unconditional branch to the instruction that already follows it.
///
/// Labels and incoming branches are retargeted to that same successor by the
/// common removal helper, so the instruction has no control-flow effect.
pub(crate) fn remove_fallthrough_branches(generator: &mut Generator) {
    while let Some(index) = fallthrough_branch(&generator.output.instructions) {
        remove_instruction_retargeting_to_next(generator, index);
    }
}

fn fallthrough_branch(instructions: &[Instruction]) -> Option<usize> {
    instructions
        .iter()
        .enumerate()
        .find_map(|(index, instruction)| {
            matches!(instruction, Instruction::Branch { target } if *target == index + 1)
                .then_some(index)
        })
}

fn forwarding_branch_block(
    instructions: &[Instruction],
    allow_unreferenced: bool,
) -> Option<(usize, usize)> {
    (1..instructions.len()).find_map(|index| {
        let Instruction::Branch { target: landing } = instructions[index] else {
            return None;
        };
        if landing <= index
            || !matches!(instructions[index - 1], Instruction::Branch { target } if target != index)
        {
            return None;
        }
        let has_incoming = instructions.iter().take(index).any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if *target == index
            )
        });
        (allow_unreferenced || has_incoming).then_some((index, landing))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        fallthrough_branch, forwarding_branch_block, thread_conditional_branch_targets,
    };
    use mwcc_machine_code::Instruction;

    #[test]
    fn recognizes_an_unreachable_forwarding_branch_block() {
        let instructions = [
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 3,
            },
            Instruction::load_immediate(3, 2),
            Instruction::Branch { target: 5 },
            Instruction::Branch { target: 4 },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(forwarding_branch_block(&instructions, false), Some((3, 4)));
    }

    #[test]
    fn recognizes_an_unreferenced_branch_after_an_unconditional_edge() {
        let instructions = [
            Instruction::load_immediate(3, 2),
            Instruction::Branch { target: 4 },
            Instruction::Branch { target: 3 },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(forwarding_branch_block(&instructions, true), Some((2, 3)));
        assert_eq!(forwarding_branch_block(&instructions, false), None);
    }

    #[test]
    fn recognizes_an_unconditional_branch_to_fallthrough() {
        let instructions = [
            Instruction::load_immediate(3, 0),
            Instruction::Branch { target: 2 },
            Instruction::Branch { target: 0 },
        ];

        assert_eq!(fallthrough_branch(&instructions), Some(1));
    }

    #[test]
    fn preserves_a_forwarding_branch_reachable_by_fallthrough() {
        let instructions = [
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 2,
            },
            Instruction::load_immediate(3, 2),
            Instruction::Branch { target: 3 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(forwarding_branch_block(&instructions, false), None);
    }

    #[test]
    fn threads_a_conditional_around_a_reachable_forwarding_branch() {
        let mut instructions = [
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 2,
            },
            Instruction::load_immediate(16, 1),
            Instruction::Branch { target: 4 },
            Instruction::load_immediate(16, 2),
            Instruction::BranchToLinkRegister,
        ];

        thread_conditional_branch_targets(&mut instructions);

        assert!(matches!(
            instructions[0],
            Instruction::BranchConditionalForward { target: 4, .. }
        ));
        assert_eq!(instructions[2], Instruction::Branch { target: 4 });
    }
}
