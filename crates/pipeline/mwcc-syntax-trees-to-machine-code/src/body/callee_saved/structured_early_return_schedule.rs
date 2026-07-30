//! Branch folding for void early returns in structured bodies.
//!
//! Semantic statement emission initially represents `if (condition) return;`
//! as a false-edge skip over an unconditional branch to the shared epilogue.
//! MWCC folds that two-branch diamond to one inverted conditional edge.

#[allow(unused_imports)]
use super::*;

/// Out-of-stream destination used until structured scheduling has finished.
///
/// Scheduling passes may insert or remove instructions and remap every branch
/// destination numerically. Keeping the marker in the upper half of `usize`
/// lets those ordinary remaps preserve its identity without tying return edges
/// to stale instruction indices.
pub(super) const STRUCTURED_EPILOGUE_PLACEHOLDER: usize = usize::MAX / 2;

fn is_structured_epilogue_placeholder(target: usize) -> bool {
    target >= STRUCTURED_EPILOGUE_PLACEHOLDER / 2
}

pub(super) fn resolve_structured_epilogue_branches(
    instructions: &mut [Instruction],
    epilogue: usize,
) {
    for instruction in instructions {
        match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. }
                if is_structured_epilogue_placeholder(*target) =>
            {
                *target = epilogue;
            }
            _ => {}
        }
    }
}

/// Frame-free structured bodies return directly from each source early-return
/// block. Conditional placeholder edges still target the final fallthrough
/// return, but an unconditional value-return edge is already a complete leaf
/// exit and becomes `blr` in place.
pub(super) fn resolve_leaf_structured_returns(
    instructions: &mut [Instruction],
    epilogue: usize,
) {
    for instruction in instructions {
        match instruction {
            Instruction::Branch { target } if is_structured_epilogue_placeholder(*target) => {
                *instruction = Instruction::BranchToLinkRegister;
            }
            Instruction::BranchConditionalForward { target, .. }
                if is_structured_epilogue_placeholder(*target) =>
            {
                *target = epilogue;
            }
            _ => {}
        }
    }
}

fn adjacent_structured_epilogue_branch(
    instructions: &[Instruction],
    epilogue: usize,
) -> Option<usize> {
    let branch = epilogue.checked_sub(1)?;
    matches!(
        instructions.get(branch),
        Some(Instruction::Branch { target }) if *target == epilogue
    )
    .then_some(branch)
}

impl Generator {
    pub(super) fn fold_structured_void_early_return_branches(&mut self) {
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
                    Instruction::Branch { target }
                        if is_structured_epilogue_placeholder(target)
                )
            {
                conditional += 1;
                continue;
            }
            let incoming: Vec<_> = self.output.instructions[..conditional]
                .iter()
                .enumerate()
                .filter_map(|(index, instruction)| {
                    matches!(
                        instruction,
                        Instruction::BranchConditionalForward { target, .. }
                            | Instruction::Branch { target }
                            if *target == return_branch
                    )
                    .then_some(index)
                })
                .collect();
            if !incoming.is_empty() {
                for branch in incoming {
                    match &mut self.output.instructions[branch] {
                        Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target } => {
                            *target = STRUCTURED_EPILOGUE_PLACEHOLDER;
                        }
                        _ => unreachable!("incoming branch was classified above"),
                    }
                }
                conditional += 2;
                continue;
            }

            self.output.instructions[conditional] = Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target: STRUCTURED_EPILOGUE_PLACEHOLDER,
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
            conditional += 1;
        }
    }

    /// Remove the unreachable second edge when a terminal `then { return; }`
    /// arm and the enclosing if/else skip both resolve to the shared epilogue.
    ///
    /// The two branches cannot be coalesced before return placeholders are
    /// resolved: one still targets zero while the other already targets the
    /// lexical join. After resolution, equal adjacent edges expose the exact
    /// redundant instruction without requiring source-shape knowledge here.
    pub(super) fn fold_adjacent_structured_epilogue_branches(&mut self) {
        let mut first = 0;
        while first + 1 < self.output.instructions.len() {
            let second = first + 1;
            let (
                Instruction::Branch {
                    target: first_target,
                },
                Instruction::Branch {
                    target: second_target,
                },
            ) = (
                &self.output.instructions[first],
                &self.output.instructions[second],
            )
            else {
                first += 1;
                continue;
            };
            if first_target != second_target
                || self.output.instructions.iter().enumerate().any(
                    |(index, instruction)| {
                        index != first
                            && index != second
                            && matches!(
                                instruction,
                                Instruction::BranchConditionalForward { target, .. }
                                    | Instruction::Branch { target }
                                    if *target == second
                            )
                    },
                )
            {
                first += 1;
                continue;
            }

            self.output.instructions.remove(second);
            self.labels.removed(second, 1);
            self.output
                .relocations
                .retain(|relocation| relocation.instruction_index != second);
            for relocation in &mut self.output.relocations {
                if relocation.instruction_index > second {
                    relocation.instruction_index -= 1;
                }
            }
            for instruction in &mut self.output.instructions {
                match instruction {
                    Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                        if *target > second =>
                    {
                        *target -= 1;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Remove an unconditional branch when the shared epilogue is already the
    /// next instruction.
    ///
    /// This must run after return placeholders resolve and before other folds
    /// can change the numeric epilogue index. Restricting the fold to the
    /// structured epilogue boundary avoids erasing intentional source-level
    /// branch identities elsewhere in the body.
    pub(super) fn fold_branch_into_adjacent_structured_epilogue(&mut self, epilogue: usize) {
        let Some(branch) =
            adjacent_structured_epilogue_branch(&self.output.instructions, epilogue)
        else {
            return;
        };
        crate::remove_instruction_retargeting_to_next(self, branch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_return_edges_by_placeholder_after_instruction_motion() {
        let mut instructions = vec![
            Instruction::Branch {
                target: STRUCTURED_EPILOGUE_PLACEHOLDER,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: STRUCTURED_EPILOGUE_PLACEHOLDER + 3,
            },
        ];

        instructions.swap(0, 1);
        resolve_structured_epilogue_branches(&mut instructions, 3);

        assert!(matches!(
            instructions[1],
            Instruction::Branch { target: 3 }
        ));
        assert!(matches!(
            instructions[2],
            Instruction::BranchConditionalForward { target: 3, .. }
        ));
    }

    #[test]
    fn resolves_leaf_value_returns_directly() {
        let mut instructions = vec![
            Instruction::Branch {
                target: STRUCTURED_EPILOGUE_PLACEHOLDER,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: STRUCTURED_EPILOGUE_PLACEHOLDER,
            },
        ];

        resolve_leaf_structured_returns(&mut instructions, 2);

        assert!(matches!(
            instructions[0],
            Instruction::BranchToLinkRegister
        ));
        assert!(matches!(
            instructions[1],
            Instruction::BranchConditionalForward { target: 2, .. }
        ));
    }

    #[test]
    fn identifies_only_an_unconditional_branch_into_the_adjacent_epilogue() {
        let adjacent = vec![
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::Branch { target: 2 },
        ];
        assert_eq!(adjacent_structured_epilogue_branch(&adjacent, 2), Some(1));

        let nonadjacent = vec![
            Instruction::Branch { target: 2 },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
        ];
        assert_eq!(adjacent_structured_epilogue_branch(&nonadjacent, 2), None);

        let conditional = vec![Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 1,
        }];
        assert_eq!(
            adjacent_structured_epilogue_branch(&conditional, 1),
            None
        );
        assert_eq!(adjacent_structured_epilogue_branch(&[], 0), None);
    }
}
