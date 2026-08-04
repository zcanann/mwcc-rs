//! Final phase-local homes and call schedule for a guarded stack walk.
//!
//! After earlier variadic phases expire, build 163 reuses their saved homes:
//! the back-chain pointer takes r25 and the reinitialized iteration bound takes
//! r26. Treating one source-level index as one whole-function home misses this
//! phase split and pushes the pointer into a later saved register.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StackTraceReportLoopPlan {
    start: usize,
    old_counter: u8,
    old_pointer: u8,
}

impl Generator {
    pub(crate) fn schedule_stack_trace_report_loop(&mut self) {
        if !self.behavior.schedule_latency_slots {
            return;
        }
        let Some(plan) = stack_trace_report_loop_plan(&self.output.instructions) else {
            return;
        };
        apply_stack_trace_report_loop_plan(&mut self.output, plan);
    }
}

fn apply_stack_trace_report_loop_plan(
    output: &mut mwcc_machine_code::MachineFunction,
    plan: StackTraceReportLoopPlan,
) {
    debug_assert_ne!(plan.old_counter, plan.old_pointer);

    basic_block_schedule::permute_contents(output, plan.start, [1, 0]);
    basic_block_schedule::permute_contents(output, plan.start + 5, [2, 1, 3, 0, 4, 5]);

    const POINTER: u8 = 25;
    const COUNTER: u8 = 26;
    let Instruction::LoadWord { d, .. } = &mut output.instructions[plan.start] else {
        unreachable!("the initial back-chain load was matched")
    };
    *d = POINTER;
    let Instruction::AddImmediate { d, .. } = &mut output.instructions[plan.start + 1] else {
        unreachable!("the counter initialization was matched")
    };
    *d = COUNTER;
    let Instruction::LoadWord { a, .. } = &mut output.instructions[plan.start + 5] else {
        unreachable!("the first report load was matched")
    };
    *a = POINTER;
    output.instructions[plan.start + 6] = Instruction::move_register(4, POINTER);
    let Instruction::LoadWord { a, .. } = &mut output.instructions[plan.start + 7] else {
        unreachable!("the second report load was matched")
    };
    *a = POINTER;
    output.instructions[plan.start + 11] = Instruction::LoadWord {
        d: POINTER,
        a: POINTER,
        offset: 0,
    };
    let Instruction::CompareLogicalWordImmediate { a, .. } =
        &mut output.instructions[plan.start + 12]
    else {
        unreachable!("the null back-chain test was matched")
    };
    *a = POINTER;
    let Instruction::AddImmediateShifted { a, .. } =
        &mut output.instructions[plan.start + 14]
    else {
        unreachable!("the sentinel back-chain test was matched")
    };
    *a = POINTER;
    let Instruction::CompareLogicalWordImmediate { a, .. } =
        &mut output.instructions[plan.start + 17]
    else {
        unreachable!("the iteration bound was matched")
    };
    *a = COUNTER;
    output.instructions[plan.start + 18] = Instruction::AddImmediate {
        d: COUNTER,
        a: COUNTER,
        immediate: 1,
    };
}

fn stack_trace_report_loop_plan(instructions: &[Instruction]) -> Option<StackTraceReportLoopPlan> {
    instructions.windows(20).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediate {
                d: counter,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: pointer,
                a: context,
                offset: 4,
            },
            Instruction::Branch { target: first_empty },
            Instruction::Branch { target: second_empty },
            Instruction::Branch { target: condition },
            Instruction::AddImmediate {
                d: 3,
                a: format_base,
                ..
            },
            Instruction::AddImmediate {
                d: 4,
                a: copied_pointer,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: first_load_pointer,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 6,
                a: second_load_pointer,
                offset: 4,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: report },
            Instruction::LoadWord {
                d: advanced_pointer,
                a: advance_base,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: null_pointer,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: first_exit,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: sentinel_pointer,
                immediate: 1,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 65535,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: second_exit,
            },
            Instruction::CompareLogicalWordImmediate {
                a: compared_counter,
                immediate: 16,
            },
            Instruction::AddImmediate {
                d: advanced_counter,
                a: advance_counter_base,
                immediate: 1,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: body,
            },
        ] = window
        else {
            return None;
        };
        ((14..=31).contains(counter)
            && (14..=31).contains(pointer)
            && counter != pointer
            && (14..=31).contains(context)
            && (14..=31).contains(format_base)
            && copied_pointer == pointer
            && first_load_pointer == pointer
            && second_load_pointer == pointer
            && advanced_pointer == pointer
            && advance_base == pointer
            && null_pointer == pointer
            && sentinel_pointer == pointer
            && compared_counter == counter
            && advanced_counter == counter
            && advance_counter_base == counter
            && report == "OSReport"
            && *first_empty == start + 3
            && *second_empty == start + 4
            && *condition == start + 12
            && *first_exit == start + 20
            && first_exit == second_exit
            && *body == start + 5)
            .then_some(StackTraceReportLoopPlan {
                start,
                old_counter: *counter,
                old_pointer: *pointer,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guarded_back_chain_report_loop() -> Vec<Instruction> {
        vec![
            Instruction::AddImmediate { d: 25, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 29, a: 28, offset: 4 },
            Instruction::Branch { target: 3 },
            Instruction::Branch { target: 4 },
            Instruction::Branch { target: 12 },
            Instruction::AddImmediate { d: 3, a: 31, immediate: 408 },
            Instruction::AddImmediate { d: 4, a: 29, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 29, offset: 0 },
            Instruction::LoadWord { d: 6, a: 29, offset: 4 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "OSReport".into() },
            Instruction::LoadWord { d: 29, a: 29, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 29, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 20,
            },
            Instruction::AddImmediateShifted { d: 0, a: 29, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 65535 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 20,
            },
            Instruction::CompareLogicalWordImmediate { a: 25, immediate: 16 },
            Instruction::AddImmediate { d: 25, a: 25, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 5,
            },
        ]
    }

    #[test]
    fn recognizes_a_guarded_back_chain_report_loop() {
        let instructions = guarded_back_chain_report_loop();
        assert_eq!(
            stack_trace_report_loop_plan(&instructions),
            Some(StackTraceReportLoopPlan {
                start: 0,
                old_counter: 25,
                old_pointer: 29,
            })
        );
    }

    #[test]
    fn assigns_phase_local_homes_without_moving_loop_boundaries() {
        let mut output = mwcc_machine_code::MachineFunction {
            instructions: guarded_back_chain_report_loop(),
            ..Default::default()
        };
        let plan = stack_trace_report_loop_plan(&output.instructions).unwrap();

        apply_stack_trace_report_loop_plan(&mut output, plan);

        assert_eq!(
            &output.instructions[..8],
            &[
                Instruction::LoadWord { d: 25, a: 28, offset: 4 },
                Instruction::AddImmediate { d: 26, a: 0, immediate: 0 },
                Instruction::Branch { target: 3 },
                Instruction::Branch { target: 4 },
                Instruction::Branch { target: 12 },
                Instruction::LoadWord { d: 5, a: 25, offset: 0 },
                Instruction::move_register(4, 25),
                Instruction::LoadWord { d: 6, a: 25, offset: 4 },
            ]
        );
        assert_eq!(
            output.instructions[11],
            Instruction::LoadWord { d: 25, a: 25, offset: 0 }
        );
        assert_eq!(
            output.instructions[12],
            Instruction::CompareLogicalWordImmediate { a: 25, immediate: 0 }
        );
        assert_eq!(
            output.instructions[17],
            Instruction::CompareLogicalWordImmediate { a: 26, immediate: 16 }
        );
        assert_eq!(
            output.instructions[18],
            Instruction::AddImmediate { d: 26, a: 26, immediate: 1 }
        );
        assert_eq!(
            output.instructions[19],
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 5,
            }
        );
    }
}
