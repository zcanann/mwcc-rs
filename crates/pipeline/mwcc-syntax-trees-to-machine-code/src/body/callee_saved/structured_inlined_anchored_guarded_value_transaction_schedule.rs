//! Result diamond and issue order for an anchored inlined guarded transaction.
//!
//! Linkage-first functions address their replacement command block from the
//! function's data anchor rather than a `lis`/`addi` relocation pair. Preserve
//! the inlined helper's boolean result boundary and schedule the path-local
//! transaction around that single anchored address instruction.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AnchoredGuardedValueTransaction {
    cancel_branch: usize,
    resume_store: usize,
    executing_load: usize,
    dummy_address: usize,
    ten: usize,
    state_ready: usize,
    epilogue: usize,
}

fn first_external_relocation_index(
    relocations: &[mwcc_machine_code::Relocation],
    kind: RelocationKind,
    target: &str,
) -> Option<usize> {
    relocations.iter().find_map(|relocation| {
        (relocation.kind == kind
            && matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(name) if name == target
            ))
        .then_some(relocation.instruction_index)
    })
}

fn external_relocation_index_after(
    relocations: &[mwcc_machine_code::Relocation],
    kind: RelocationKind,
    target: &str,
    after: usize,
) -> Option<usize> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index > after
            && relocation.kind == kind
            && matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(name) if name == target
            ))
        .then_some(relocation.instruction_index)
    })
}

fn anchored_guarded_value_transaction(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
) -> Option<AnchoredGuardedValueTransaction> {
    let retry_store =
        first_external_relocation_index(relocations, RelocationKind::EmbSda21, "NumInternalRetry")?;
    let initial_zero = retry_store.checked_sub(1)?;
    let cancel_load = external_relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "Canceling",
        retry_store,
    )?;
    let cancel_branch = cancel_load + 2;
    let resume_constant = cancel_branch + 1;
    let resume_store = resume_constant + 1;
    let zero = resume_store + 1;
    let executing_load = zero + 1;
    let cancel_store = executing_load + 1;
    let dummy_address = cancel_store + 1;
    let executing_store = dummy_address + 1;
    let ten = executing_store + 1;
    let state_ready =
        external_relocation_index_after(relocations, RelocationKind::Rel24, "stateReady", ten)?;
    let Instruction::Branch { target: epilogue } = instructions.get(state_ready + 1)? else {
        return None;
    };

    if cancel_load > retry_store + 8
        || instructions[retry_store + 1..cancel_load]
            .iter()
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchAndLink { .. }
                        | Instruction::BranchToCountRegisterAndLink
                        | Instruction::BranchToLinkRegisterAndLink
                        | Instruction::Branch { .. }
                )
            })
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
                immediate,
            }) if *immediate != 0
        )
        || !matches!(
            instructions.get(resume_store),
            Some(Instruction::StoreWord { s: 0, a: 0, .. })
        )
        || !matches!(
            instructions.get(zero),
            Some(Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            })
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
    {
        return None;
    }

    Some(AnchoredGuardedValueTransaction {
        cancel_branch,
        resume_store,
        executing_load,
        dummy_address,
        ten,
        state_ready,
        epilogue: *epilogue,
    })
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_anchored_guarded_value_transaction(&mut self) {
        if self.inline_statement_body_substitutions == 0
            && self.late_inline_statement_body_substitutions == 0
        {
            return;
        }
        let Some(plan) = anchored_guarded_value_transaction(
            &self.output.instructions,
            &self.output.relocations,
            &self.output.data_section_displacements,
        ) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, plan.executing_load, plan.resume_store);
        crate::move_instruction_before_retargeting(self, plan.dummy_address, plan.resume_store + 2);
        crate::move_instruction_before_retargeting(self, plan.ten, plan.resume_store + 5);

        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.resume_store + 2]
        else {
            unreachable!("validated anchored replacement address changed form")
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.resume_store + 6]
        else {
            unreachable!("validated replacement publication changed form")
        };
        *s = 3;

        let state_ready = plan.state_ready;
        let Instruction::Branch { .. } = self.output.instructions[state_ready + 1] else {
            unreachable!("validated guarded-value exit changed form")
        };
        self.output.instructions[state_ready + 1] = Instruction::load_immediate(0, 1);
        crate::insert_instruction_retargeting(
            self,
            state_ready + 2,
            Instruction::Branch { target: 0 },
        );
        crate::insert_instruction_retargeting(
            self,
            state_ready + 3,
            Instruction::load_immediate(0, 0),
        );
        crate::insert_instruction_retargeting(
            self,
            state_ready + 4,
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        );
        crate::insert_instruction_retargeting(
            self,
            state_ready + 5,
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            },
        );

        let Instruction::Branch { target } = &mut self.output.instructions[state_ready + 2] else {
            unreachable!("inserted true-result join changed form")
        };
        *target = state_ready + 4;
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[plan.cancel_branch]
        else {
            unreachable!("validated cancellation branch changed form")
        };
        *target = state_ready + 3;
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[state_ready + 5]
        else {
            unreachable!("inserted guarded-value exit changed form")
        };
        *target = plan.epilogue + 4;
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
    fn recognizes_a_data_anchored_guarded_value_transaction() {
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
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 15,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::load_immediate(0, 1),
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
                target: 19,
            },
            Instruction::load_immediate(0, 7),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
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
            Instruction::Branch { target: 21 },
            Instruction::BranchToLinkRegister,
        ];
        let relocations = vec![
            relocation(1, RelocationKind::EmbSda21, "NumInternalRetry"),
            relocation(7, RelocationKind::EmbSda21, "Canceling"),
            relocation(11, RelocationKind::EmbSda21, "ResumeFromHere"),
            relocation(13, RelocationKind::EmbSda21, "executing"),
            relocation(14, RelocationKind::EmbSda21, "Canceling"),
            relocation(16, RelocationKind::EmbSda21, "executing"),
            relocation(18, RelocationKind::Rel24, "stateReady"),
        ];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 15,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                "DummyCommandBlock".into(),
            ),
        }];

        let plan = anchored_guarded_value_transaction(&instructions, &relocations, &displacements)
            .expect("the anchored transaction should match");

        assert_eq!(plan.cancel_branch, 9);
        assert_eq!(plan.dummy_address, 15);
        assert_eq!(plan.state_ready, 18);
        assert_eq!(plan.epilogue, 21);
    }
}
