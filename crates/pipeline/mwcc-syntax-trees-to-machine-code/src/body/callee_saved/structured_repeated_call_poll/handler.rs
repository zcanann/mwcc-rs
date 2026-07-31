//! Late schedules for the Revolution DSP interrupt-handler protocol.
//!
//! The source recognizer owns the broad repeated call/poll transaction.  This
//! module additionally requires the handler's fixed-address entry and named
//! task globals before applying its switch-arm schedule.

use mwcc_machine_code::{Instruction, Relocation, RelocationTarget};

use crate::Generator;

impl Generator {
    /// Share the fixed-address bank, preserve switch exits, and reuse loaded
    /// task pointers after allocation has exposed MWCC's handler schedule.
    pub(crate) fn schedule_structured_call_poll_fixed_address_entry(&mut self) {
        if !self.structured_repeated_call_poll_owner || !has_fixed_address_entry(self) {
            return;
        }

        schedule_fixed_address_entry(self);
        insert_terminal_callback_exit(self);
        schedule_entry_callback_reset(self);
        schedule_rude_task_handoff(self);
        schedule_rude_task_completion(self);
        schedule_first_task_completion(self);
        schedule_next_task_completion(self);
        fold_resume_next_task_load_and_insert_exit(self);
        retarget_early_case_exits(self);
        retarget_completion_case_exits(self);
        normalize_context_pointer_offsets(self);
    }
}

fn has_fixed_address_entry(generator: &Generator) -> bool {
    matches!(
        generator.output.instructions.as_slice(),
        [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, .. },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::Or { a: 31, s: 4, b: 4 },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::LoadHalfwordZero { d: 4, a: 4, .. },
            Instruction::AddImmediate { d: 0, a: 0, .. },
            Instruction::And { a: 0, s: 4, b: 0 },
            Instruction::OrImmediate { a: 4, s: 0, .. },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::StoreHalfword { s: 4, a: 3, .. },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 16
            },
            Instruction::BranchAndLink { .. },
            ..
        ]
    )
}

fn schedule_fixed_address_entry(generator: &mut Generator) {
    let Instruction::AddImmediateShifted { d, .. } = &mut generator.output.instructions[5] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *d = 6;
    let Instruction::LoadHalfwordZero { d, a, .. } = &mut generator.output.instructions[6] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *d = 5;
    *a = 6;
    let Instruction::And { s, .. } = &mut generator.output.instructions[8] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *s = 5;
    let Instruction::OrImmediate { a, .. } = &mut generator.output.instructions[9] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *a = 0;
    let Instruction::StoreHalfword { s, a, .. } = &mut generator.output.instructions[11] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *s = 0;
    *a = 6;

    crate::remove_instruction_retargeting_to_next(generator, 10);
    let Instruction::AddImmediate { immediate, .. } = &mut generator.output.instructions[11] else {
        unreachable!("the fixed-address entry was validated above")
    };
    *immediate = 8;
    crate::move_instruction_before_retargeting(generator, 5, 2);
    crate::move_instruction_before_retargeting(generator, 7, 4);
    crate::move_instruction_before_retargeting(generator, 11, 5);
}

fn insert_terminal_callback_exit(generator: &mut Generator) {
    let Some(callback_prefix) = generator.output.instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchAndLink { .. },
                Instruction::LoadWord {
                    d: 12,
                    a: 5,
                    offset: 52
                },
                Instruction::CompareWordImmediate {
                    a: 12,
                    immediate: 0
                },
                Instruction::BranchConditionalForward { .. },
                Instruction::Or { a: 3, s: 5, b: 5 },
                Instruction::MoveToCountRegister { s: 12 },
                Instruction::BranchToCountRegisterAndLink,
                Instruction::AddImmediate {
                    d: 3,
                    a: 1,
                    immediate: 8 | 16
                }
            ]
        )
    }) else {
        return;
    };
    let callback = callback_prefix + 1;
    crate::insert_instruction_retargeting(
        generator,
        callback,
        Instruction::Branch {
            target: callback + 6,
        },
    );
}

fn schedule_entry_callback_reset(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::LoadWord {
                    d: 12,
                    a: 3,
                    offset: 44
                }
            ]
        )
    }) else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start + 1, "__DSP_rude_task")
        || !relocation_named(&generator.output.relocations, start + 2, "__DSP_curr_task")
        || !relocation_named(
            &generator.output.relocations,
            start + 3,
            "__DSP_rude_task_pending",
        )
    {
        return;
    }
    crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
}

fn schedule_rude_task_handoff(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(7).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 2
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 0
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                }
            ]
        )
    }) else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start + 2, "__DSP_rude_task")
        || !relocation_named(&generator.output.relocations, start + 3, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 5, "__DSP_rude_task")
        || !relocation_named(
            &generator.output.relocations,
            start + 6,
            "__DSP_rude_task_pending",
        )
    {
        return;
    }

    let Instruction::AddImmediate { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!("the rude-task handoff was validated above")
    };
    *d = 4;
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[start + 1] else {
        unreachable!("the rude-task handoff was validated above")
    };
    *s = 4;
    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start + 2] else {
        unreachable!("the rude-task handoff was validated above")
    };
    *d = 3;
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[start + 3] else {
        unreachable!("the rude-task handoff was validated above")
    };
    *s = 3;
    crate::move_instruction_before_retargeting(generator, start + 4, start + 1);
    crate::move_instruction_before_retargeting(generator, start + 6, start + 4);
}

fn schedule_rude_task_completion(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0
                }
            ]
        )
    }) else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start, "__DSP_rude_task")
        || !relocation_named(&generator.output.relocations, start + 1, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 3, "__DSP_rude_task")
        || !relocation_named(
            &generator.output.relocations,
            start + 4,
            "__DSP_rude_task_pending",
        )
    {
        return;
    }

    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!("the rude-task completion was validated above")
    };
    *d = 3;
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[start + 1] else {
        unreachable!("the rude-task completion was validated above")
    };
    *s = 3;
    crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
    crate::move_instruction_before_retargeting(generator, start + 4, start + 2);
}

fn schedule_first_task_completion(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 3
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 0
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 0,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 0
                },
                Instruction::BranchAndLink { .. }
            ]
        )
    }) else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 3, "__DSP_first_task")
        || !relocation_named(&generator.output.relocations, start + 5, "__DSP_exec_task")
    {
        return;
    }

    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!("the first-task completion was validated above")
    };
    *d = 4;
    let Instruction::StoreWord { a, .. } = &mut generator.output.instructions[start + 2] else {
        unreachable!("the first-task completion was validated above")
    };
    *a = 4;
    crate::move_instruction_before_retargeting(generator, start + 4, start + 2);
}

fn schedule_next_task_completion(generator: &mut Generator) {
    let Some(start) = generator.output.instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 3,
                    a: 0,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 3
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 0
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 0
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 0,
                    offset: 0
                }
            ]
        )
    }) else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 4, "__DSP_curr_task")
    {
        return;
    }

    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!("the next-task completion was validated above")
    };
    *d = 4;
    let Instruction::StoreWord { a, .. } = &mut generator.output.instructions[start + 2] else {
        unreachable!("the next-task completion was validated above")
    };
    *a = 4;
    crate::move_instruction_before_retargeting(generator, start + 3, start + 2);
}

fn fold_resume_next_task_load_and_insert_exit(generator: &mut Generator) {
    let Some(start) = generator
        .output
        .instructions
        .windows(11)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord {
                        d: 3,
                        a: 0,
                        offset: 0
                    },
                    Instruction::LoadWord {
                        d: 4,
                        a: 0,
                        offset: 0
                    },
                    Instruction::LoadWord {
                        d: 4,
                        a: 4,
                        offset: 56
                    },
                    Instruction::BranchAndLink { .. },
                    Instruction::LoadWord {
                        d: 3,
                        a: 0,
                        offset: 0
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 2
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: 3,
                        offset: 0
                    },
                    Instruction::LoadWord {
                        d: 3,
                        a: 0,
                        offset: 0
                    },
                    Instruction::LoadWord {
                        d: 0,
                        a: 3,
                        offset: 56
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: 0,
                        offset: 0
                    },
                    Instruction::LoadWord {
                        d: 0,
                        a: 0,
                        offset: 0
                    }
                ]
            )
        })
    else {
        return;
    };
    if !relocation_named(&generator.output.relocations, start, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 1, "__DSP_curr_task")
        || !relocation_named(&generator.output.relocations, start + 3, "__DSP_exec_task")
    {
        return;
    }

    let Instruction::LoadWord { a, .. } = &mut generator.output.instructions[start + 2] else {
        unreachable!("the next-task load was validated above")
    };
    *a = 3;
    crate::remove_instruction_retargeting_to_next(generator, start + 1);

    let exit = start + 9;
    let Some(epilogue) = context_epilogue(generator) else {
        return;
    };
    crate::insert_instruction_retargeting(
        generator,
        exit,
        Instruction::Branch { target: epilogue },
    );
}

fn retarget_early_case_exits(generator: &mut Generator) {
    let Some(next_case) = generator
        .output
        .instructions
        .windows(4)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord {
                        d: 0,
                        a: 0,
                        offset: 0
                    },
                    Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                    Instruction::BranchConditionalForward { .. },
                    Instruction::LoadWord {
                        d: 12,
                        a: 5,
                        offset: 48
                    }
                ]
            )
        })
        .filter(|&index| {
            relocation_named(
                &generator.output.relocations,
                index,
                "__DSP_rude_task_pending",
            )
        })
    else {
        return;
    };
    let Some(epilogue) = context_epilogue(generator) else {
        return;
    };
    retarget_all_but_dispatch(generator, next_case, epilogue, 7);
}

fn retarget_completion_case_exits(generator: &mut Generator) {
    let Some(terminal_callback) = generator.output.instructions.windows(3).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 12,
                    a: 5,
                    offset: 52
                },
                Instruction::CompareWordImmediate {
                    a: 12,
                    immediate: 0
                },
                Instruction::BranchConditionalForward { .. }
            ]
        )
    }) else {
        return;
    };
    let Some(epilogue) = context_epilogue(generator) else {
        return;
    };
    retarget_all_but_dispatch(generator, terminal_callback, epilogue, 4);
}

fn retarget_all_but_dispatch(
    generator: &mut Generator,
    old_target: usize,
    epilogue: usize,
    expected_branches: usize,
) {
    let exits = generator
        .output
        .instructions
        .iter()
        .enumerate()
        .take(old_target)
        .filter_map(|(index, instruction)| match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target == old_target =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if exits.len() != expected_branches {
        return;
    }
    for index in exits.into_iter().skip(1) {
        match &mut generator.output.instructions[index] {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => *target = epilogue,
            _ => unreachable!("the case exit was collected above"),
        }
    }
}

fn normalize_context_pointer_offsets(generator: &mut Generator) {
    for instruction in &mut generator.output.instructions {
        if let Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 16,
        } = instruction
        {
            *instruction = Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            };
        }
    }
}

fn context_epilogue(generator: &Generator) -> Option<usize> {
    generator
        .output
        .instructions
        .iter()
        .rposition(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 3,
                    a: 1,
                    immediate: 8 | 16
                }
            )
        })
}

fn relocation_named(relocations: &[Relocation], instruction_index: usize, expected: &str) -> bool {
    relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction_index
            && matches!(&relocation.target, RelocationTarget::External(target) if target == expected)
    })
}
