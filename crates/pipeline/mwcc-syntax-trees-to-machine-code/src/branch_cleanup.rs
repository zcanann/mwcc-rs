//! Late cleanup of structured control-flow forwarding blocks.

use crate::{Generator, remove_instruction_retargeting_to_next};
use mwcc_machine_code::Instruction;

/// Thread conditional branches through an otherwise unreachable one-branch
/// forwarding block, then remove that dead block.
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

    while let Some((index, landing)) =
        forwarding_branch_block(&generator.output.instructions)
    {
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

fn forwarding_branch_block(instructions: &[Instruction]) -> Option<(usize, usize)> {
    (1..instructions.len()).find_map(|index| {
        let Instruction::Branch { target: landing } = instructions[index] else {
            return None;
        };
        if landing <= index
            || !matches!(instructions[index - 1], Instruction::Branch { target } if target != index)
        {
            return None;
        }
        let conditional_incoming = instructions
            .iter()
            .take(index)
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchConditionalForward { target, .. } if *target == index
                )
            });
        conditional_incoming.then_some((index, landing))
    })
}

#[cfg(test)]
mod tests {
    use super::forwarding_branch_block;
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

        assert_eq!(forwarding_branch_block(&instructions), Some((3, 4)));
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

        assert_eq!(forwarding_branch_block(&instructions), None);
    }
}
