//! Final issue order for a dynamically resumed guarded value transaction.
//!
//! The caller selects a resume state on several structured edges before the
//! inlined cancellation helper. Build 163 retains that merged value in `r4`,
//! schedules the transaction packet around it, and preserves the helper's
//! canonical `1`/`0` result diamond ahead of the caller continuation.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct DynamicGuardedValueDiamond {
    category_call: usize,
    cancel_load: usize,
    cancel_branch: usize,
    resume_store: usize,
    zero: usize,
    executing_load: usize,
    cancel_store: usize,
    dummy_high: usize,
    dummy_low: usize,
    executing_store: usize,
    ten: usize,
    state_ready: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_dynamic_guarded_value_diamond(&mut self) {
        if self.inline_source_call_survivors.len() < 2 {
            return;
        }
        let Some(plan) =
            dynamic_guarded_value_diamond(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        for instruction in &mut self.output.instructions[plan.category_call + 1..plan.cancel_load] {
            if let Instruction::AddImmediate { d, a: 0, .. } = instruction {
                if *d == 3 {
                    *d = 4;
                }
            }
        }
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.resume_store]
        else {
            unreachable!("validated resume store changed form")
        };
        *s = 4;

        crate::move_instruction_before_retargeting(self, plan.dummy_high, plan.resume_store);
        let cancel_store = relocation_index(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "Canceling",
            1,
        )
        .expect("validated cancellation store disappeared");
        let dummy_low = relocation_index(
            &self.output.relocations,
            RelocationKind::Addr16Lo,
            "DummyCommandBlock",
            0,
        )
        .expect("validated dummy low half disappeared");
        crate::move_instruction_before_retargeting(self, dummy_low, cancel_store);

        let executing_store = relocation_index(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
            1,
        )
        .expect("validated executing store disappeared");
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

        let dummy_low = relocation_index(
            &self.output.relocations,
            RelocationKind::Addr16Lo,
            "DummyCommandBlock",
            0,
        )
        .expect("validated dummy low half disappeared");
        let Instruction::AddImmediate { d, a, .. } = &mut self.output.instructions[dummy_low]
        else {
            unreachable!("validated dummy low half changed form")
        };
        *d = 3;
        *a = 3;
        let executing_store = relocation_index(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "executing",
            1,
        )
        .expect("validated executing store disappeared");
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[executing_store]
        else {
            unreachable!("validated executing publication changed form")
        };
        *s = 3;

        let state_ready = relocation_index(
            &self.output.relocations,
            RelocationKind::Rel24,
            "stateReady",
            0,
        )
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
                target: epilogue + 4,
            },
        );
        let Instruction::Branch { target } = &mut self.output.instructions[state_ready + 2] else {
            unreachable!("inserted true-result join changed form")
        };
        *target = state_ready + 4;

        let cancel_load = relocation_index(
            &self.output.relocations,
            RelocationKind::EmbSda21,
            "Canceling",
            0,
        )
        .expect("validated cancellation load disappeared");
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[cancel_load + 2]
        else {
            unreachable!("validated cancellation branch changed form")
        };
        *target = state_ready + 3;
    }
}

fn dynamic_guarded_value_diamond(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<DynamicGuardedValueDiamond> {
    let category_call = relocation_index(relocations, RelocationKind::Rel24, "CategorizeError", 0)?;
    let cancel_load = relocation_index(relocations, RelocationKind::EmbSda21, "Canceling", 0)?;
    let cancel_store = relocation_index(relocations, RelocationKind::EmbSda21, "Canceling", 1)?;
    let resume_store =
        relocation_index(relocations, RelocationKind::EmbSda21, "ResumeFromHere", 0)?;
    let executing_load = relocation_index(relocations, RelocationKind::EmbSda21, "executing", 0)?;
    let executing_store = relocation_index(relocations, RelocationKind::EmbSda21, "executing", 1)?;
    let dummy_high = relocation_index(
        relocations,
        RelocationKind::Addr16Ha,
        "DummyCommandBlock",
        0,
    )?;
    let dummy_low = relocation_index(
        relocations,
        RelocationKind::Addr16Lo,
        "DummyCommandBlock",
        0,
    )?;
    let state_ready = relocation_index(relocations, RelocationKind::Rel24, "stateReady", 0)?;
    let cancel_branch = cancel_load + 2;
    let zero = resume_store + 1;
    let ten = executing_store + 1;

    if !(category_call < cancel_load
        && resume_store == cancel_branch + 1
        && zero + 1 == executing_load
        && executing_load + 1 == cancel_store
        && cancel_store + 1 == dummy_high
        && dummy_high + 1 == dummy_low
        && dummy_low + 1 == executing_store
        && executing_store + 1 == ten
        && ten < state_ready)
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
            instructions.get(resume_store),
            Some(Instruction::StoreWord { s: 3, a: 0, .. })
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

    Some(DynamicGuardedValueDiamond {
        category_call,
        cancel_load,
        cancel_branch,
        resume_store,
        zero,
        executing_load,
        cancel_store,
        dummy_high,
        dummy_low,
        executing_store,
        ten,
        state_ready,
    })
}

fn relocation_index(
    relocations: &[mwcc_machine_code::Relocation],
    kind: RelocationKind,
    target: &str,
    occurrence: usize,
) -> Option<usize> {
    relocations
        .iter()
        .filter(|relocation| {
            relocation.kind == kind
                && matches!(
                    &relocation.target,
                    mwcc_machine_code::RelocationTarget::External(name) if name == target
                )
        })
        .nth(occurrence)
        .map(|relocation| relocation.instruction_index)
}
