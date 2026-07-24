//! Branch folding for void early returns in structured bodies.
//!
//! Semantic statement emission initially represents `if (condition) return;`
//! as a false-edge skip over an unconditional branch to the shared epilogue.
//! MWCC folds that two-branch diamond to one inverted conditional edge.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn fold_structured_void_early_return_branches(
        &mut self,
        return_branches: &mut Vec<usize>,
    ) {
        let mut conditional = 0;
        while conditional + 1 < self.output.instructions.len() {
            let return_branch = conditional + 1;
            let Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target,
            } = self.output.instructions[conditional]
            else {
                conditional += 1;
                continue;
            };
            if target != return_branch + 1
                || !matches!(
                    self.output.instructions[return_branch],
                    Instruction::Branch { target: 0 }
                )
                || !return_branches.contains(&return_branch)
            {
                conditional += 1;
                continue;
            }

            self.output.instructions[conditional] = Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target: 0,
            };
            self.output.instructions.remove(return_branch);
            self.labels.removed(return_branch, 1);
            self.output
                .relocations
                .retain(|relocation| relocation.instruction_index != return_branch);
            for relocation in &mut self.output.relocations {
                if relocation.instruction_index > return_branch {
                    relocation.instruction_index -= 1;
                }
            }
            for instruction in &mut self.output.instructions {
                match instruction {
                    Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                        if *target > return_branch =>
                    {
                        *target -= 1;
                    }
                    _ => {}
                }
            }
            for branch in return_branches.iter_mut() {
                if *branch == return_branch {
                    *branch = conditional;
                } else if *branch > return_branch {
                    *branch -= 1;
                }
            }
            conditional += 1;
        }
    }
}
