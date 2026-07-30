//! Scheduling for a nested callback guard after resetting its outer flag.
//!
//! Once the outer flag is known nonzero, Build 163 loads the nested callback
//! before publishing the flag reset. This preserves volatile store order while
//! overlapping the independent callback load.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_nested_reset_callback(&mut self) {
        let Some(start) = nested_reset_callback(&self.output) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 5, start + 3);
    }
}

fn nested_reset_callback(output: &mwcc_machine_code::MachineFunction) -> Option<usize> {
    let start = output.instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: tested,
                    a: 0,
                    offset: 0,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: compared,
                    immediate: 0,
                }
                | Instruction::CompareWordImmediate {
                    a: compared,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: outer_target,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 12,
                    a: 0,
                    offset: 0,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: 12,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: inner_target,
                },
            ] if tested == compared && outer_target == inner_target
        )
    })?;
    let relocations = &output.relocations;
    let constants = &output.constants;
    if !super::super::schedule_relocations::same_relocated_value(
        relocations,
        constants,
        start,
        start + 4,
    ) || super::super::schedule_relocations::same_target_value(
        relocations,
        constants,
        start,
        start + 5,
    ) {
        return None;
    }
    Some(start)
}
