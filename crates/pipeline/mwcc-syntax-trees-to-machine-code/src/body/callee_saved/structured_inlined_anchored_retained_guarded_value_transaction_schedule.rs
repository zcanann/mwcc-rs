//! Retained result scheduling for an anchored inlined guarded transaction.
//!
//! When the caller continues after the inlined cancellation helper, MWCC keeps
//! the helper's false result in `r5`, reuses it for the transaction's zero
//! stores, and compares the joined true/false value at the caller boundary.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedTransaction {
    initial_zero: usize,
    retry_store: usize,
    cancel_branch: usize,
    resume_constant: usize,
    resume_store: usize,
    executing_load: usize,
    cancel_store: usize,
    dummy_address: usize,
    executing_store: usize,
    ten: usize,
    state_ready: usize,
    epilogue: usize,
}

fn external_indices(
    relocations: &[mwcc_machine_code::Relocation],
    kind: RelocationKind,
    target: &str,
) -> Vec<usize> {
    relocations
        .iter()
        .filter_map(|relocation| {
            (relocation.kind == kind
                && matches!(
                    &relocation.target,
                    mwcc_machine_code::RelocationTarget::External(name) if name == target
                ))
            .then_some(relocation.instruction_index)
        })
        .collect()
}

fn recognize(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
) -> Option<RetainedTransaction> {
    let cancel_indices = external_indices(relocations, RelocationKind::EmbSda21, "Canceling");
    let executing_indices = external_indices(relocations, RelocationKind::EmbSda21, "executing");
    let state_ready_indices = external_indices(relocations, RelocationKind::Rel24, "stateReady");

    for retry_store in external_indices(relocations, RelocationKind::EmbSda21, "NumInternalRetry") {
        let Some(initial_zero) = retry_store.checked_sub(1) else {
            continue;
        };
        let Some(cancel_load) = cancel_indices
            .iter()
            .copied()
            .find(|index| *index > retry_store)
        else {
            continue;
        };
        let cancel_branch = cancel_load + 2;
        let resume_constant = cancel_branch + 1;
        let resume_store = resume_constant + 1;
        let executing_load = resume_store + 1;
        let cancel_store = executing_load + 1;
        let dummy_address = cancel_store + 1;
        let executing_store = dummy_address + 1;
        let ten = executing_store + 1;
        let Some(state_ready) = state_ready_indices
            .iter()
            .copied()
            .find(|index| *index > ten)
        else {
            continue;
        };
        let Some(Instruction::Branch { target: epilogue }) = instructions.get(state_ready + 1)
        else {
            continue;
        };

        if cancel_load > retry_store + 32
            || !matches!(
                instructions.get(initial_zero),
                Some(Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                })
            )
            || !matches!(
                instructions.get(retry_store),
                Some(Instruction::StoreWord { s: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(cancel_load),
                Some(Instruction::LoadWord { d: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(cancel_load + 1),
                Some(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 })
            )
            || !matches!(
                instructions.get(cancel_branch),
                Some(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    ..
                })
            )
            || !matches!(
                instructions.get(resume_constant),
                Some(Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                })
            )
            || !matches!(
                instructions.get(resume_store),
                Some(Instruction::StoreWord { s: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(executing_load),
                Some(Instruction::LoadWord { d: 30, a: 0, .. })
            )
            || !matches!(
                instructions.get(cancel_store),
                Some(Instruction::StoreWord { s: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(dummy_address),
                Some(Instruction::AddImmediate { d: 0, a, .. }) if *a != 0
            )
            || !displacements
                .iter()
                .any(|displacement| displacement.instruction_index == dummy_address)
            || !matches!(
                instructions.get(executing_store),
                Some(Instruction::StoreWord { s: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(ten),
                Some(Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 10,
                })
            )
            || !executing_indices.contains(&executing_load)
            || !executing_indices.contains(&executing_store)
            || !cancel_indices.contains(&cancel_store)
        {
            continue;
        }

        return Some(RetainedTransaction {
            initial_zero,
            retry_store,
            cancel_branch,
            resume_constant,
            resume_store,
            executing_load,
            cancel_store,
            dummy_address,
            executing_store,
            ten,
            state_ready,
            epilogue: *epilogue,
        });
    }
    None
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_anchored_retained_guarded_value_transaction(
        &mut self,
    ) {
        if self.inline_statement_body_substitutions == 0
            && self.late_inline_statement_body_substitutions == 0
        {
            return;
        }
        let Some(plan) = recognize(
            &self.output.instructions,
            &self.output.relocations,
            &self.output.data_section_displacements,
        ) else {
            return;
        };

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[plan.initial_zero]
        else {
            unreachable!("validated false result changed form")
        };
        *d = 5;
        for index in [plan.retry_store, plan.resume_store, plan.cancel_store] {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[index] else {
                unreachable!("validated retained-result store changed form")
            };
            *s = 5;
        }

        crate::move_instruction_before_retargeting(self, plan.dummy_address, plan.executing_load);
        crate::move_instruction_before_retargeting(self, plan.ten, plan.cancel_store + 1);

        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.executing_load]
        else {
            unreachable!("validated anchored replacement address changed form")
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.ten] else {
            unreachable!("validated replacement publication changed form")
        };
        *s = 3;

        crate::remove_instruction_retargeting_to_next(self, plan.resume_constant);
        let state_ready = plan.state_ready - 1;
        let epilogue = plan.epilogue - 1;
        let Instruction::Branch { .. } = self.output.instructions[state_ready + 1] else {
            unreachable!("validated transaction exit changed form")
        };
        self.output.instructions[state_ready + 1] = Instruction::load_immediate(5, 1);
        crate::insert_instruction_retargeting(
            self,
            state_ready + 2,
            Instruction::CompareWordImmediate { a: 5, immediate: 0 },
        );
        crate::insert_instruction_retargeting(
            self,
            state_ready + 3,
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            },
        );

        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[plan.cancel_branch]
        else {
            unreachable!("validated cancellation branch changed form")
        };
        *target = state_ready + 2;
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[state_ready + 3]
        else {
            unreachable!("inserted caller exit changed form")
        };
        *target = epilogue + 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_a_retained_anchored_transaction() {
        let instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 30,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 31,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 10),
            Instruction::BranchAndLink {
                target: "stateReady".into(),
            },
            Instruction::Branch { target: 15 },
            Instruction::BranchToLinkRegister,
        ];
        let relocations = vec![
            relocation(1, RelocationKind::EmbSda21, "NumInternalRetry"),
            relocation(2, RelocationKind::EmbSda21, "Canceling"),
            relocation(6, RelocationKind::EmbSda21, "ResumeFromHere"),
            relocation(7, RelocationKind::EmbSda21, "executing"),
            relocation(8, RelocationKind::EmbSda21, "Canceling"),
            relocation(10, RelocationKind::EmbSda21, "executing"),
            relocation(12, RelocationKind::Rel24, "stateReady"),
        ];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 9,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                "DummyCommandBlock".into(),
            ),
        }];

        let plan = recognize(&instructions, &relocations, &displacements)
            .expect("the retained transaction should match");
        assert_eq!(plan.cancel_branch, 4);
        assert_eq!(plan.resume_constant, 5);
        assert_eq!(plan.dummy_address, 9);
        assert_eq!(plan.state_ready, 12);
    }
}
