//! Reuse a guarded pointer member load as the guarded call's first argument.
//!
//! Selection can allocate a null-test-only value to r0 and then reload the
//! identical member into r3 on the taken path.  Nothing can mutate the member
//! between those operations, so MWCC keeps the first load in r3 for both uses.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_guarded_call_pointer_loads(&mut self) {
        while let Some((load, reload)) =
            guarded_call_pointer_reload(&self.output.instructions)
        {
            if self.preserve_guarded_named_local_values {
                preserve_guarded_call_pointer_value(
                    &mut self.output.instructions,
                    load,
                    reload,
                );
                continue;
            }
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[load] else {
                unreachable!("the guarded pointer load was matched")
            };
            *d = Eabi::general_result().number;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[load + 1]
            else {
                unreachable!("the guarded pointer comparison was matched")
            };
            *a = Eabi::general_result().number;
            crate::remove_instruction_retargeting_to_next(self, reload);
        }
        if self.preserve_guarded_named_local_values {
            self.expand_guarded_named_local_calls();
        }
        while let Some(plan) = guarded_indirect_callback_reload(&self.output) {
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.load] else {
                unreachable!("the guarded callback load was matched")
            };
            *d = 12;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[plan.load + 1]
            else {
                unreachable!("the guarded callback comparison was matched")
            };
            *a = 12;
            crate::remove_instruction_retargeting_to_next(self, plan.reload);
            let mtlr = plan.mtlr - 1;
            let guarded_entry = plan.load + 3;
            if mtlr > guarded_entry {
                crate::move_instruction_before_retargeting(self, mtlr, guarded_entry);
            }
            normalize_two_argument_guarded_callback(self, guarded_entry);
        }
    }

    fn expand_guarded_named_local_calls(&mut self) {
        let mut start = 0;
        while start < self.output.instructions.len().saturating_sub(3) {
            if !is_direct_guarded_pointer_call(&self.output.instructions, start) {
                start += 1;
                continue;
            }
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start] else {
                unreachable!("the direct guarded pointer load was matched")
            };
            *d = GENERAL_SCRATCH;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[start + 1]
            else {
                unreachable!("the direct guarded pointer comparison was matched")
            };
            *a = GENERAL_SCRATCH;
            crate::insert_instruction_retargeting(
                self,
                start + 2,
                Instruction::move_register(Eabi::general_result().number, GENERAL_SCRATCH),
            );
            start += 5;
        }
    }
}

fn preserve_guarded_call_pointer_value(
    instructions: &mut [Instruction],
    load: usize,
    reload: usize,
) {
    let Instruction::LoadWord { d: source, .. } = instructions[load] else {
        unreachable!("the guarded pointer load was matched")
    };
    instructions[reload] = Instruction::move_register(Eabi::general_result().number, source);
}

fn guarded_call_pointer_reload(instructions: &[Instruction]) -> Option<(usize, usize)> {
    instructions.windows(5).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::LoadWord {
                    d: tested,
                    a: tested_base,
                    offset: tested_offset,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: compared,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: argument_base,
                    offset: argument_offset,
                },
                Instruction::BranchAndLink { .. },
            ] if tested == compared
                && *tested != 3
                && tested_base == argument_base
                && tested_offset == argument_offset
                && *target == start + 5 =>
            {
                Some((start, start + 3))
            }
            _ => None,
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuardedIndirectCallbackReload {
    load: usize,
    reload: usize,
    mtlr: usize,
}

fn guarded_indirect_callback_reload(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<GuardedIndirectCallbackReload> {
    let instructions = &output.instructions;
    instructions.windows(3).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord {
                d: tested,
                a: tested_base,
                offset: tested_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: compared,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            },
        ] = window
        else {
            return None;
        };
        if tested != compared || *tested == 12 {
            return None;
        }
        let search_end = (*target)
            .min(start.saturating_add(11))
            .min(instructions.len());
        let reload = (start + 3..search_end).find(|&index| {
            matches!(
                instructions[index],
                Instruction::LoadWord {
                    d: 12,
                    a,
                    offset,
                } if a == *tested_base
                    && offset == *tested_offset
                    && same_load_identity(output, start, index)
            )
        })?;
        let mtlr = (reload + 1..instructions.len().saturating_sub(1))
            .take(8)
            .find(|&index| {
                matches!(
                    instructions[index..index + 2],
                    [
                        Instruction::MoveToLinkRegister { s: 12 },
                        Instruction::BranchToLinkRegisterAndLink,
                    ]
                )
            })?;
        if *target != mtlr + 2
            || instructions[reload + 1..mtlr].iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchAndLink { .. }
                        | Instruction::BranchToLinkRegisterAndLink
                        | Instruction::BranchToCountRegisterAndLink
                        | Instruction::BranchConditionalForward { .. }
                        | Instruction::Branch { .. }
                ) || mwcc_vreg::register_operands(instruction)
                    .iter()
                    .any(|operand| {
                        operand.class == mwcc_vreg::Class::General
                            && operand.role == mwcc_vreg::RegisterRole::Define
                            && operand.register == 12
                    })
            })
        {
            return None;
        }
        Some(GuardedIndirectCallbackReload {
            load: start,
            reload,
            mtlr,
        })
    })
}

fn same_load_identity(
    output: &mwcc_machine_code::MachineFunction,
    left: usize,
    right: usize,
) -> bool {
    super::super::schedule_relocations::same_relocated_or_unpatched_value(
        &output.relocations,
        &output.constants,
        left,
        right,
    )
}

/// The global-pointer callback family materializes its two scalar arguments
/// after `mtlr`, from the higher ABI register down. A saved-register copy uses
/// `addi ...,0` here instead of the allocator's ordinary `mr` spelling.
fn normalize_two_argument_guarded_callback(generator: &mut Generator, mtlr: usize) {
    let Some(call) = (mtlr + 1..generator.output.instructions.len()).find(|&index| {
        matches!(
            generator.output.instructions[index],
            Instruction::BranchToLinkRegisterAndLink
        )
    }) else {
        return;
    };
    if call != mtlr + 3 {
        return;
    }

    let destinations = [
        callback_argument_destination(&generator.output.instructions[mtlr + 1]),
        callback_argument_destination(&generator.output.instructions[mtlr + 2]),
    ];
    if destinations == [Some(3), Some(4)] {
        crate::move_instruction_before_retargeting(generator, mtlr + 2, mtlr + 1);
    } else if destinations != [Some(4), Some(3)] {
        return;
    }

    if let Instruction::Or { a: 4, s, b } = generator.output.instructions[mtlr + 1] {
        if s == b {
            generator.output.instructions[mtlr + 1] = Instruction::AddImmediate {
                d: 4,
                a: s,
                immediate: 0,
            };
        }
    }
}

fn callback_argument_destination(instruction: &Instruction) -> Option<u8> {
    match instruction {
        Instruction::AddImmediate { d, .. } => Some(*d),
        Instruction::Or { a, s, b } if s == b => Some(*a),
        _ => None,
    }
}

fn is_direct_guarded_pointer_call(instructions: &[Instruction], start: usize) -> bool {
    matches!(
        instructions.get(start..start + 4),
        Some([
            Instruction::LoadWord { d: 3, .. },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            },
            Instruction::BranchAndLink { .. },
        ]) if *target >= start + 4
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guarded_reload(target: usize) -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 0,
                a: 30,
                offset: 20,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            },
            Instruction::LoadWord {
                d: 3,
                a: 30,
                offset: 20,
            },
            Instruction::BranchAndLink {
                target: "release".into(),
            },
        ]
    }

    #[test]
    fn recognizes_a_reload_guarded_around_one_call() {
        assert_eq!(guarded_call_pointer_reload(&guarded_reload(5)), Some((0, 3)));
    }

    #[test]
    fn preserves_a_reload_when_the_branch_has_another_join() {
        assert_eq!(guarded_call_pointer_reload(&guarded_reload(6)), None);
    }

    #[test]
    fn forwards_a_named_guard_value_instead_of_reloading_it() {
        let mut instructions = guarded_reload(5);
        preserve_guarded_call_pointer_value(&mut instructions, 0, 3);

        assert_eq!(instructions[3], Instruction::move_register(3, 0));
        assert_eq!(guarded_call_pointer_reload(&instructions), None);
    }

    #[test]
    fn recognizes_a_direct_guarded_pointer_call_for_named_local_expansion() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 28,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert!(is_direct_guarded_pointer_call(&instructions, 0));
    }

    #[test]
    fn recognizes_a_reloaded_member_callback_with_argument_setup() {
        let mut output = mwcc_machine_code::MachineFunction::new("callback");
        output.instructions = vec![
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 40,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 7,
            },
            Instruction::LoadWord {
                d: 12,
                a: 4,
                offset: 40,
            },
            Instruction::load_immediate(3, 0),
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];

        assert_eq!(
            guarded_indirect_callback_reload(&output),
            Some(GuardedIndirectCallbackReload {
                load: 0,
                reload: 3,
                mtlr: 5,
            })
        );
    }

    #[test]
    fn recognizes_a_global_callback_reloaded_after_two_arguments() {
        let mut output = mwcc_machine_code::MachineFunction::new("callback");
        output.instructions = vec![
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::load_immediate(3, 0),
            Instruction::move_register(4, 31),
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];

        assert_eq!(
            guarded_indirect_callback_reload(&output),
            Some(GuardedIndirectCallbackReload {
                load: 0,
                reload: 5,
                mtlr: 6,
            })
        );
    }

    #[test]
    fn does_not_reuse_a_different_relocated_global_with_the_same_encoding() {
        let mut output = mwcc_machine_code::MachineFunction::new("callback");
        output.instructions = vec![
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::load_immediate(3, 0),
            Instruction::move_register(4, 31),
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];
        for (instruction_index, target) in [(0, "enabled"), (5, "callback")] {
            output.relocations.push(mwcc_machine_code::Relocation {
                instruction_index,
                kind: RelocationKind::EmbSda21,
                target: mwcc_machine_code::RelocationTarget::External(target.into()),
            });
        }

        assert_eq!(guarded_indirect_callback_reload(&output), None);
    }
}
