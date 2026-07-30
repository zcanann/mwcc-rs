//! Final issue order for an inlined guarded transaction with a value diamond.
//!
//! When the caller has already occupied the natural false-result register,
//! Build 163 materializes `1` and `0` on separate edges in `r0`, then joins
//! those edges for the caller's comparison. This is distinct from the retained
//! `r4` transaction schedule.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GuardedValueDiamond {
    cancel_branch: usize,
    resume_constant: usize,
    executing_load: usize,
    dummy_high: usize,
    dummy_low: usize,
    executing_store: usize,
    ten: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_guarded_value_diamond(&mut self) {
        if self.inline_statement_body_substitutions == 0 {
            return;
        }
        let Some(plan) = guarded_value_diamond(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        let start = plan.resume_constant;
        crate::move_instruction_before_retargeting(self, plan.executing_load, start + 1);
        crate::move_instruction_before_retargeting(self, plan.dummy_high, start + 2);
        crate::move_instruction_before_retargeting(self, plan.dummy_low, start + 4);
        crate::move_instruction_before_retargeting(self, plan.executing_store, start + 6);
        crate::move_instruction_before_retargeting(self, plan.ten, start + 7);

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start + 5] else {
            unreachable!("validated transaction zero changed form")
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 8] else {
            unreachable!("validated cancel store changed form")
        };
        *s = 3;

        let state_ready = call_index(&self.output.relocations, "stateReady")
            .expect("validated stateReady call disappeared");
        let Instruction::Branch { target: epilogue } = self.output.instructions[state_ready + 1]
        else {
            unreachable!("validated transaction exit changed form")
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
            unreachable!("validated cancel branch changed form")
        };
        *target = state_ready + 3;
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[state_ready + 5]
        else {
            unreachable!("inserted caller guard changed form")
        };
        *target = epilogue + 4;
    }
}

fn guarded_value_diamond(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<GuardedValueDiamond> {
    let cancel_load = first_relocation_index(relocations, RelocationKind::EmbSda21, "Canceling")?;
    let resume_store = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "ResumeFromHere",
        cancel_load,
    )?;
    let executing_load = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "executing",
        resume_store,
    )?;
    let cancel_store = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "Canceling",
        cancel_load,
    )?;
    let dummy_high = relocation_index_after(
        relocations,
        RelocationKind::Addr16Ha,
        "DummyCommandBlock",
        cancel_store,
    )?;
    let dummy_low = relocation_index_after(
        relocations,
        RelocationKind::Addr16Lo,
        "DummyCommandBlock",
        dummy_high,
    )?;
    let executing_store = relocation_index_after(
        relocations,
        RelocationKind::EmbSda21,
        "executing",
        executing_load,
    )?;
    let state_ready = call_index(relocations, "stateReady")?;
    let cancel_branch = cancel_load + 2;
    let resume_constant = cancel_branch + 1;
    let zero = resume_store + 1;
    let ten = executing_store + 1;

    if resume_store != resume_constant + 1
        || zero + 1 != executing_load
        || executing_load + 1 != cancel_store
        || cancel_store + 1 != dummy_high
        || dummy_high + 1 != dummy_low
        || dummy_low + 1 != executing_store
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
        || !matches!(
            instructions.get(ten),
            Some(Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 10,
            })
        )
        || !matches!(
            instructions.get(state_ready + 1),
            Some(Instruction::Branch { .. })
        )
    {
        return None;
    }

    Some(GuardedValueDiamond {
        cancel_branch,
        resume_constant,
        executing_load,
        dummy_high,
        dummy_low,
        executing_store,
        ten,
    })
}

fn first_relocation_index(
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
    first_relocation_index(relocations, RelocationKind::Rel24, target)
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
    fn recognizes_a_guarded_transaction_with_separate_result_edges() {
        let instructions = vec![
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
            Instruction::load_immediate(0, 2),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
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
            Instruction::Branch { target: 15 },
            Instruction::load_immediate(0, 11),
            Instruction::BranchToLinkRegister,
        ];
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "Canceling"),
            relocation(4, RelocationKind::EmbSda21, "ResumeFromHere"),
            relocation(6, RelocationKind::EmbSda21, "executing"),
            relocation(7, RelocationKind::EmbSda21, "Canceling"),
            relocation(8, RelocationKind::Addr16Ha, "DummyCommandBlock"),
            relocation(9, RelocationKind::Addr16Lo, "DummyCommandBlock"),
            relocation(10, RelocationKind::EmbSda21, "executing"),
            relocation(12, RelocationKind::Rel24, "stateReady"),
        ];

        let plan = guarded_value_diamond(&instructions, &relocations)
            .expect("the complete result diamond should match");

        assert_eq!(plan.cancel_branch, 2);
        assert_eq!(plan.resume_constant, 3);
        assert_eq!(plan.executing_load, 6);
        assert_eq!(plan.executing_store, 10);
    }
}
