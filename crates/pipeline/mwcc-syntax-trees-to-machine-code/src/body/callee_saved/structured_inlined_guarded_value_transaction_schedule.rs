//! Final value lifetime and issue order for an inlined guarded transaction.
//!
//! Build 163 keeps the transaction's false result in `r4`, reuses it for the
//! transaction's zero stores, and joins the true and false results before the
//! caller's continuation. Selection initially splices the true edge directly
//! into the caller and branches around that continuation.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GuardedValueTransaction {
    initial_zero: usize,
    retry_store: usize,
    cancel_branch: usize,
    resume_constant: usize,
    resume_store: usize,
    redundant_zero: Option<usize>,
    executing_load: usize,
    cancel_store: usize,
    dummy_high: usize,
    dummy_low: usize,
    executing_store: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_guarded_value_transaction(&mut self) {
        if self.inline_statement_body_substitutions == 0
            && self.late_inline_statement_body_substitutions == 0
        {
            return;
        }
        let Some(plan) =
            guarded_value_transaction(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[plan.initial_zero]
        else {
            unreachable!("validated false result changed form")
        };
        *d = 4;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.retry_store]
        else {
            unreachable!("validated retry store changed form")
        };
        *s = 4;

        let resume_is_zero = matches!(
            self.output.instructions[plan.resume_constant],
            Instruction::AddImmediate { immediate: 0, .. }
        );
        let removed = if resume_is_zero {
            plan.resume_constant
        } else {
            plan.redundant_zero
                .expect("a nonzero resume value has a separate transaction zero")
        };
        crate::remove_instruction_retargeting_to_next(self, removed);

        let resume_store = adjusted_index(plan.resume_store, removed);
        let executing_load = adjusted_index(plan.executing_load, removed);
        let cancel_store = adjusted_index(plan.cancel_store, removed);
        let dummy_high = adjusted_index(plan.dummy_high, removed);
        let dummy_low = adjusted_index(plan.dummy_low, removed);
        let executing_store = adjusted_index(plan.executing_store, removed);

        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[resume_store] else {
            unreachable!("validated resume store changed form")
        };
        if resume_is_zero {
            *s = 4;
        }
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[cancel_store] else {
            unreachable!("validated cancel store changed form")
        };
        *s = 4;
        let Instruction::AddImmediate { d, a, .. } = &mut self.output.instructions[dummy_low]
        else {
            unreachable!("validated dummy address low half changed form")
        };
        *d = 3;
        *a = 3;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[executing_store]
        else {
            unreachable!("validated executing publication changed form")
        };
        *s = 3;

        if resume_is_zero {
            crate::move_instruction_before_retargeting(self, dummy_high, executing_load);
            crate::move_instruction_before_retargeting(self, dummy_low, cancel_store + 1);
        } else {
            crate::move_instruction_before_retargeting(self, executing_load, resume_store);
            crate::move_instruction_before_retargeting(self, dummy_high, cancel_store);
            crate::move_instruction_before_retargeting(self, dummy_low, cancel_store + 1);
        }

        let dummy_low = relocation_index(
            &self.output.relocations,
            RelocationKind::Addr16Lo,
            "DummyCommandBlock",
        )
        .expect("validated dummy address low half disappeared");
        let executing_store = relocation_index_after(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
            dummy_low,
        )
        .expect("validated executing publication disappeared");
        let ten = self.output.instructions[executing_store + 1..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 10,
                    }
                )
            })
            .map(|relative| executing_store + 1 + relative)
            .expect("validated command-state constant disappeared");
        crate::move_instruction_before_retargeting(self, ten, executing_store);

        let state_ready = call_index(&self.output.relocations, "stateReady")
            .expect("validated stateReady call disappeared");
        let Instruction::Branch { target: epilogue } = self.output.instructions[state_ready + 1]
        else {
            unreachable!("validated transaction exit changed form")
        };
        self.output.instructions[state_ready + 1] = Instruction::load_immediate(4, 1);
        crate::insert_instruction_retargeting(
            self,
            state_ready + 2,
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
        );
        crate::insert_instruction_retargeting(
            self,
            state_ready + 3,
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: epilogue + 1,
            },
        );

        let cancel_branch = adjusted_index(plan.cancel_branch, removed);
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[cancel_branch]
        else {
            unreachable!("validated cancel branch changed form")
        };
        *target = state_ready + 2;
    }
}

fn adjusted_index(index: usize, removed: usize) -> usize {
    index - usize::from(index > removed)
}

fn guarded_value_transaction(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<GuardedValueTransaction> {
    let retry_store = relocation_index(relocations, RelocationKind::EmbSda21, "NumInternalRetry")?;
    let initial_zero = retry_store.checked_sub(1)?;
    if !matches!(
        instructions.get(initial_zero),
        Some(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        })
    ) || !matches!(
        instructions.get(retry_store),
        Some(Instruction::StoreWord { s: 0, a: 0, .. })
    ) {
        return None;
    }

    let cancel_load = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "Canceling",
        retry_store,
    )?;
    let cancel_store = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "Canceling",
        cancel_load,
    )?;
    let resume_store = relocation_index(relocations, RelocationKind::EmbSda21, "ResumeFromHere")?;
    let executing_load = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "executing",
        resume_store,
    )?;
    let executing_store = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "executing",
        executing_load,
    )?;
    let dummy_high = relocation_index(relocations, RelocationKind::Addr16Ha, "DummyCommandBlock")?;
    let dummy_low = relocation_index(relocations, RelocationKind::Addr16Lo, "DummyCommandBlock")?;
    let state_ready = call_index(relocations, "stateReady")?;
    let cancel_branch = cancel_load + 2;
    let resume_constant = resume_store.checked_sub(1)?;
    let epilogue = match instructions.get(state_ready + 1)? {
        Instruction::Branch { target } => *target,
        _ => return None,
    };

    if cancel_load != retry_store + 1
        || resume_store <= cancel_branch
        || resume_store > cancel_branch + 2
        || executing_load > resume_store + 2
        || cancel_store != executing_load + 1
        || dummy_high != cancel_store + 1
        || dummy_low != dummy_high + 1
        || executing_store != dummy_low + 1
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
            Some(Instruction::AddImmediate { d: 0, a: 0, .. })
        )
        || !matches!(
            instructions.get(resume_store),
            Some(Instruction::StoreWord { s: 0, a: 0, .. })
        )
        || !matches!(
            instructions.get(executing_load),
            Some(Instruction::LoadWord { d: 31, a: 0, .. })
        )
        || !matches!(
            instructions.get(cancel_store),
            Some(Instruction::StoreWord { s: 0, a: 0, .. })
        )
        || !matches!(
            instructions.get(dummy_high),
            Some(Instruction::AddImmediateShifted { d: 3, a: 0, .. })
        )
        || !matches!(
            instructions.get(dummy_low),
            Some(Instruction::AddImmediate { d: 0, a: 3, .. })
        )
        || !matches!(
            instructions.get(executing_store),
            Some(Instruction::StoreWord { s: 0, a: 0, .. })
        )
    {
        return None;
    }

    let redundant_zero = if resume_store + 1 == executing_load {
        None
    } else if matches!(
        instructions.get(resume_store + 1),
        Some(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        })
    ) {
        Some(resume_store + 1)
    } else {
        return None;
    };

    Some(GuardedValueTransaction {
        initial_zero,
        retry_store,
        cancel_branch,
        resume_constant,
        resume_store,
        redundant_zero,
        executing_load,
        cancel_store,
        dummy_high,
        dummy_low,
        executing_store,
    })
}

fn relocation_index(
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

fn relocation_index_after(
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

fn call_index(relocations: &[mwcc_machine_code::Relocation], target: &str) -> Option<usize> {
    relocation_index(relocations, RelocationKind::Rel24, target)
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
    fn recognizes_a_relocated_guarded_value_transaction() {
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
                target: 15,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 31,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate_shifted(3, 0),
            Instruction::AddImmediate {
                d: 0,
                a: 3,
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
            Instruction::Branch { target: 16 },
            Instruction::load_immediate(0, 6),
            Instruction::BranchToLinkRegister,
        ];
        let relocations = vec![
            relocation(1, RelocationKind::EmbSda21, "NumInternalRetry"),
            relocation(2, RelocationKind::EmbSda21, "Canceling"),
            relocation(6, RelocationKind::EmbSda21, "ResumeFromHere"),
            relocation(7, RelocationKind::EmbSda21, "executing"),
            relocation(8, RelocationKind::EmbSda21, "Canceling"),
            relocation(9, RelocationKind::Addr16Ha, "DummyCommandBlock"),
            relocation(10, RelocationKind::Addr16Lo, "DummyCommandBlock"),
            relocation(11, RelocationKind::EmbSda21, "executing"),
            relocation(13, RelocationKind::Rel24, "stateReady"),
        ];

        let plan = guarded_value_transaction(&instructions, &relocations)
            .expect("the complete transaction graph should match");

        assert_eq!(plan.initial_zero, 0);
        assert_eq!(plan.cancel_branch, 4);
        assert_eq!(plan.resume_store, 6);
        assert_eq!(plan.executing_store, 11);
    }
}
