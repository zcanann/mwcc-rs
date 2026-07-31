//! Retained-result scheduling when the caller preloads the transaction object.
//!
//! A caller can compare members of the current object immediately before an
//! inlined guarded value transaction. Build 163 keeps that object in `r30`
//! across both regions and still materializes the helper's `1`/`0` result
//! diamond. The ordinary anchored transaction starts with its own global load,
//! so this preloaded form has a separate recognizer and finalizer.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PreloadedRetainedTransaction {
    executing_load: usize,
    first_member: usize,
    second_member: usize,
    cancel_load: usize,
    cancel_branch: usize,
    result_zero: usize,
    resume_store: usize,
    cancel_store: usize,
    saved_copy: usize,
    dummy_address: usize,
    executing_store: usize,
    ten: usize,
    state_ready: usize,
    epilogue: usize,
    second_executing_load: usize,
    second_dummy_address: usize,
    second_executing_store: usize,
    second_callback_link: usize,
    second_callback_result: usize,
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

fn has_symbol_displacement(
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    index: usize,
    target: &str,
) -> bool {
    displacements.iter().any(|displacement| {
        displacement.instruction_index == index
            && matches!(
                &displacement.target,
                mwcc_machine_code::DataSectionDisplacementTarget::Symbol(name)
                    if name == target
            )
    })
}

fn recognize(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
) -> Option<PreloadedRetainedTransaction> {
    let executing = external_indices(relocations, RelocationKind::EmbSda21, "executing");
    let canceling = external_indices(relocations, RelocationKind::EmbSda21, "Canceling");
    let resume = external_indices(relocations, RelocationKind::EmbSda21, "ResumeFromHere");
    let state_ready = external_indices(relocations, RelocationKind::Rel24, "stateReady");

    for cancel_load in canceling.iter().copied() {
        let Some(executing_load) = cancel_load.checked_sub(5) else {
            continue;
        };
        let first_member = executing_load + 1;
        let second_member = executing_load + 2;
        let cancel_branch = cancel_load + 2;
        let result_zero = cancel_branch + 1;
        let resume_store = result_zero + 1;
        let cancel_store = resume_store + 1;
        let saved_copy = cancel_store + 1;
        let dummy_address = saved_copy + 1;
        let executing_store = dummy_address + 1;
        let ten = executing_store + 1;
        let Some(state_ready) = state_ready.iter().copied().find(|index| *index > ten) else {
            continue;
        };
        let Some(Instruction::Branch { target: epilogue }) = instructions.get(state_ready + 1)
        else {
            continue;
        };
        let Some(second_executing_load) =
            executing.iter().copied().find(|index| *index > state_ready)
        else {
            continue;
        };
        let second_dummy_address = second_executing_load + 1;
        let second_executing_store = second_executing_load + 3;
        let second_callback_link = second_executing_load + 8;
        let second_callback_result = second_executing_load + 9;
        let second_callback_call = second_executing_load + 10;
        let second_state_ready = second_executing_load + 11;

        if !executing.contains(&executing_load)
            || !executing.contains(&executing_store)
            || !executing.contains(&second_executing_load)
            || !executing.contains(&second_executing_store)
            || !resume.contains(&resume_store)
            || canceling.iter().copied().find(|index| *index > cancel_load) != Some(cancel_store)
            || !state_ready
                .checked_add(1)
                .is_some_and(|branch| branch < instructions.len())
            || !state_ready
                .checked_sub(ten)
                .is_some_and(|distance| distance < 32)
            || !external_indices(relocations, RelocationKind::Rel24, "stateReady")
                .contains(&second_state_ready)
            || !matches!(
                instructions.get(executing_load),
                Some(Instruction::LoadWord { d: 4, a: 0, .. })
            )
            || !matches!(
                instructions.get(first_member),
                Some(Instruction::LoadWord {
                    d: 3,
                    a: 4,
                    offset: 32,
                })
            )
            || !matches!(
                instructions.get(second_member),
                Some(Instruction::LoadWord {
                    d: 0,
                    a: 4,
                    offset: 20,
                })
            )
            || !matches!(
                instructions.get(executing_load + 3),
                Some(Instruction::CompareLogicalWord { a: 3, b: 0 })
            )
            || !matches!(
                instructions.get(executing_load + 4),
                Some(Instruction::BranchConditionalForward { .. })
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
                    target,
                }) if *target == second_executing_load
            )
            || !matches!(
                instructions.get(result_zero),
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
                instructions.get(cancel_store),
                Some(Instruction::StoreWord { s: 0, a: 0, .. })
            )
            || !matches!(
                instructions.get(saved_copy),
                Some(Instruction::AddImmediate {
                    d: 30,
                    a: 4,
                    immediate: 0,
                })
            )
            || !matches!(
                instructions.get(dummy_address),
                Some(Instruction::AddImmediate { d: 0, a: 31, .. })
            )
            || !has_symbol_displacement(displacements, dummy_address, "DummyCommandBlock")
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
            || second_executing_load != state_ready + 2
            || !matches!(
                instructions.get(second_executing_load),
                Some(Instruction::LoadWord { d: 4, a: 0, .. })
            )
            || !matches!(
                instructions.get(second_dummy_address),
                Some(Instruction::AddImmediate { d: 5, a: 31, .. })
            )
            || !has_symbol_displacement(displacements, second_dummy_address, "DummyCommandBlock")
            || !matches!(
                instructions.get(second_executing_store),
                Some(Instruction::StoreWord { s: 5, a: 0, .. })
            )
            || !matches!(
                instructions.get(second_callback_link),
                Some(Instruction::MoveToLinkRegister { s: 12 })
            )
            || !matches!(
                instructions.get(second_callback_result),
                Some(Instruction::LoadWord {
                    d: 3,
                    a: 4,
                    offset: 32,
                })
            )
            || !matches!(
                instructions.get(second_callback_call),
                Some(Instruction::BranchToLinkRegisterAndLink)
            )
        {
            continue;
        }

        return Some(PreloadedRetainedTransaction {
            executing_load,
            first_member,
            second_member,
            cancel_load,
            cancel_branch,
            result_zero,
            resume_store,
            cancel_store,
            saved_copy,
            dummy_address,
            executing_store,
            ten,
            state_ready,
            epilogue: *epilogue,
            second_executing_load,
            second_dummy_address,
            second_executing_store,
            second_callback_link,
            second_callback_result,
        });
    }
    None
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_preloaded_retained_guarded_value_transaction(
        &mut self,
    ) {
        if !self.behavior.schedule_latency_slots
            || (self.inline_statement_body_substitutions == 0
                && self.late_inline_statement_body_substitutions == 0)
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

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.executing_load]
        else {
            unreachable!("validated preloaded object changed form")
        };
        *d = 30;
        for index in [plan.first_member, plan.second_member] {
            let Instruction::LoadWord { a, .. } = &mut self.output.instructions[index] else {
                unreachable!("validated preloaded member read changed form")
            };
            *a = 30;
        }
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[plan.result_zero]
        else {
            unreachable!("validated false result changed form")
        };
        *d = 4;
        for index in [plan.resume_store, plan.cancel_store] {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[index] else {
                unreachable!("validated retained-result store changed form")
            };
            *s = 4;
        }

        crate::remove_instruction_retargeting_to_next(self, plan.saved_copy);
        let dummy_address = plan.dummy_address - 1;
        crate::move_instruction_before_retargeting(self, dummy_address, plan.cancel_store);
        let cancel_store = external_indices(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "Canceling",
        )
        .into_iter()
        .find(|index| *index > plan.cancel_load)
        .expect("validated cancellation store disappeared");
        let executing_store = external_indices(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
        )
        .into_iter()
        .find(|index| *index > cancel_store)
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
            .expect("validated transaction state constant disappeared");
        crate::move_instruction_before_retargeting(self, ten, cancel_store);

        let dummy_address = self
            .output
            .data_section_displacements
            .iter()
            .find_map(|displacement| {
                (displacement.instruction_index > plan.resume_store
                    && matches!(
                        &displacement.target,
                        mwcc_machine_code::DataSectionDisplacementTarget::Symbol(name)
                            if name == "DummyCommandBlock"
                    ))
                .then_some(displacement.instruction_index)
            })
            .expect("validated replacement address disappeared");
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[dummy_address]
        else {
            unreachable!("validated replacement address changed form")
        };
        *d = 3;
        let executing_store = external_indices(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
        )
        .into_iter()
        .find(|index| *index > dummy_address)
        .expect("validated executing publication disappeared");
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[executing_store]
        else {
            unreachable!("validated executing publication changed form")
        };
        *s = 3;

        let state_ready = external_indices(
            &self.output.relocations,
            RelocationKind::Rel24,
            "stateReady",
        )
        .into_iter()
        .find(|index| *index > executing_store)
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
            unreachable!("validated cancellation branch changed form")
        };
        *target = state_ready + 3;
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[state_ready + 5]
        else {
            unreachable!("inserted caller exit changed form")
        };
        *target = epilogue + 4;

        let second_executing_load = external_indices(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
        )
        .into_iter()
        .find(|index| *index > state_ready)
        .expect("validated completion object load disappeared");
        let second_dummy_address = self
            .output
            .data_section_displacements
            .iter()
            .find_map(|displacement| {
                (displacement.instruction_index > second_executing_load
                    && matches!(
                        &displacement.target,
                        mwcc_machine_code::DataSectionDisplacementTarget::Symbol(name)
                            if name == "DummyCommandBlock"
                    ))
                .then_some(displacement.instruction_index)
            })
            .expect("validated completion replacement address disappeared");
        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[second_dummy_address]
        else {
            unreachable!("validated completion replacement address changed form")
        };
        *d = 3;
        let second_executing_store = external_indices(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
        )
        .into_iter()
        .find(|index| *index > second_executing_load)
        .expect("validated completion publication disappeared");
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[second_executing_store]
        else {
            unreachable!("validated completion publication changed form")
        };
        *s = 3;

        let second_state_ready = external_indices(
            &self.output.relocations,
            RelocationKind::Rel24,
            "stateReady",
        )
        .into_iter()
        .find(|index| *index > second_executing_store)
        .expect("validated completion stateReady call disappeared");
        let callback_call = self.output.instructions[second_executing_store..second_state_ready]
            .iter()
            .rposition(|instruction| {
                matches!(instruction, Instruction::BranchToLinkRegisterAndLink)
            })
            .map(|relative| second_executing_store + relative)
            .expect("validated completion callback disappeared");
        crate::move_instruction_before_retargeting(self, callback_call - 1, callback_call - 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{
        DataSectionDisplacement, DataSectionDisplacementTarget, Relocation, RelocationTarget,
    };

    fn relocation(index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_a_preloaded_retained_transaction() {
        let mut instructions = vec![Instruction::BranchToLinkRegister; 33];
        instructions[0] = Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        };
        instructions[1] = Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: 32,
        };
        instructions[2] = Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: 20,
        };
        instructions[3] = Instruction::CompareLogicalWord { a: 3, b: 0 };
        instructions[4] = Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 31,
        };
        instructions[5] = Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        };
        instructions[6] = Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 };
        instructions[7] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 18,
        };
        instructions[8] = Instruction::load_immediate(0, 0);
        instructions[9] = Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        };
        instructions[10] = Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        };
        instructions[11] = Instruction::AddImmediate {
            d: 30,
            a: 4,
            immediate: 0,
        };
        instructions[12] = Instruction::AddImmediate {
            d: 0,
            a: 31,
            immediate: 64,
        };
        instructions[13] = Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        };
        instructions[14] = Instruction::load_immediate(0, 10);
        instructions[16] = Instruction::BranchAndLink {
            target: "stateReady".into(),
        };
        instructions[17] = Instruction::Branch { target: 32 };
        instructions[18] = Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        };
        instructions[19] = Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 64,
        };
        instructions[21] = Instruction::StoreWord {
            s: 5,
            a: 0,
            offset: 0,
        };
        instructions[26] = Instruction::MoveToLinkRegister { s: 12 };
        instructions[27] = Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: 32,
        };
        instructions[28] = Instruction::BranchToLinkRegisterAndLink;
        instructions[29] = Instruction::BranchAndLink {
            target: "stateReady".into(),
        };
        instructions[30] = Instruction::Branch { target: 32 };
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "executing"),
            relocation(5, RelocationKind::EmbSda21, "Canceling"),
            relocation(9, RelocationKind::EmbSda21, "ResumeFromHere"),
            relocation(10, RelocationKind::EmbSda21, "Canceling"),
            relocation(13, RelocationKind::EmbSda21, "executing"),
            relocation(16, RelocationKind::Rel24, "stateReady"),
            relocation(18, RelocationKind::EmbSda21, "executing"),
            relocation(21, RelocationKind::EmbSda21, "executing"),
            relocation(29, RelocationKind::Rel24, "stateReady"),
        ];
        let displacements = vec![
            DataSectionDisplacement {
                instruction_index: 12,
                target: DataSectionDisplacementTarget::Symbol("DummyCommandBlock".into()),
            },
            DataSectionDisplacement {
                instruction_index: 19,
                target: DataSectionDisplacementTarget::Symbol("DummyCommandBlock".into()),
            },
        ];

        let plan = recognize(&instructions, &relocations, &displacements)
            .expect("the preloaded retained transaction should match");
        assert_eq!(plan.executing_load, 0);
        assert_eq!(plan.saved_copy, 11);
        assert_eq!(plan.state_ready, 16);
        assert_eq!(plan.second_callback_result, 27);
    }
}
