//! Build-163 scheduling for calls through register-resident function pointers.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Schedule a register-resident function pointer across build 163's entire
    /// linkage prefix. The ordinary LR-save scheduler may place the linkage
    /// store before or after argument setup, so identify the semantic pieces
    /// instead of depending on their incoming indices.
    pub(crate) fn normalize_linkage_first_indirect_call_schedule(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || self.frame_size != 8
        {
            return;
        }

        let Some(call) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::BranchToLinkRegisterAndLink)
        }) else {
            return;
        };
        let prefix = &self.output.instructions[..=call];
        let Some(link_read) = unique_position(prefix, |instruction| {
            matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 })
        }) else {
            return;
        };
        let Some(link_store) = unique_position(prefix, |instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                }
            )
        }) else {
            return;
        };
        let Some(stack_update) = unique_position(prefix, |instruction| {
            matches!(
                instruction,
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -8
                }
            )
        }) else {
            return;
        };
        let Some(pointer_copy) = unique_position(prefix, |instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 12,
                    immediate: 0,
                    ..
                }
            )
        }) else {
            return;
        };
        let Some(call_setup) = unique_position(prefix, |instruction| {
            matches!(instruction, Instruction::MoveToLinkRegister { s: 12 })
        }) else {
            return;
        };

        // A guard may establish the frame before reaching the pointer call.
        // In that shape build 163 issues mtlr immediately after preserving the
        // pointer, then materializes every argument before blrl.
        let guarded_call = stack_update < pointer_copy
            && prefix[..pointer_copy].iter().any(|instruction| {
                matches!(instruction, Instruction::BranchConditionalForward { .. })
            });
        if guarded_call {
            if call_setup <= pointer_copy
                || self
                    .output
                    .relocations
                    .iter()
                    .any(|relocation| (pointer_copy..=call).contains(&relocation.instruction_index))
            {
                return;
            }
            let mut arguments = Vec::new();
            for (index, instruction) in prefix[pointer_copy + 1..call].iter().enumerate() {
                let absolute_index = pointer_copy + 1 + index;
                if absolute_index == call_setup {
                    continue;
                }
                let Some(argument) = legacy_argument_materialization(instruction) else {
                    return;
                };
                arguments.push(argument);
            }
            let mut scheduled = Vec::with_capacity(call - pointer_copy + 1);
            scheduled.push(prefix[pointer_copy].clone());
            scheduled.push(prefix[call_setup].clone());
            scheduled.extend(arguments);
            scheduled.push(prefix[call].clone());
            self.output
                .instructions
                .splice(pointer_copy..=call, scheduled);
            return;
        }

        // This pass only permutes a relocation-free register-call prefix.
        // Global function pointers contain a relocated load and intentionally
        // retain the ordinary linkage schedule.
        if self
            .output
            .relocations
            .iter()
            .any(|relocation| relocation.instruction_index <= call)
        {
            return;
        }

        let structural = [
            link_read,
            link_store,
            stack_update,
            pointer_copy,
            call_setup,
            call,
        ];
        let mut arguments = Vec::new();
        for (index, instruction) in prefix.iter().enumerate() {
            if structural.contains(&index) {
                continue;
            }
            let Some(argument) = legacy_argument_materialization(instruction) else {
                return;
            };
            arguments.push(argument);
        }

        let mut scheduled = Vec::with_capacity(prefix.len());
        scheduled.push(prefix[link_read].clone());
        scheduled.push(prefix[pointer_copy].clone());
        scheduled.push(prefix[link_store].clone());
        let all_literals = arguments
            .iter()
            .all(|instruction| matches!(instruction, Instruction::AddImmediate { a: 0, .. }));
        if all_literals {
            // Literal arguments have no incoming-register dependency. Build
            // 163 issues mtlr first, then alternates the first literal with
            // the stack update before materializing any remaining literals.
            scheduled.push(prefix[call_setup].clone());
            if let Some((first, remaining)) = arguments.split_first() {
                scheduled.push(first.clone());
                scheduled.push(prefix[stack_update].clone());
                scheduled.extend(remaining.iter().cloned());
            } else {
                scheduled.push(prefix[stack_update].clone());
            }
        } else {
            // Register-to-register argument copies are dependency scheduled
            // evenly around mtlr, with the stack update immediately before
            // the call.
            let split = arguments.len() / 2;
            scheduled.extend(arguments[..split].iter().cloned());
            scheduled.push(prefix[call_setup].clone());
            scheduled.extend(arguments[split..].iter().cloned());
            scheduled.push(prefix[stack_update].clone());
        }
        scheduled.push(prefix[call].clone());
        debug_assert_eq!(scheduled.len(), prefix.len());
        self.output.instructions.splice(..=call, scheduled);
    }

    /// Schedule a global function-pointer tail that forwards a retained
    /// receiver beside a relocated callback argument. The pointer load issues
    /// first; its latency is filled by the callback address, then `mtlr`, and
    /// the receiver enters r3 immediately before the indirect call.
    pub(crate) fn schedule_linkage_first_global_indirect_callback_tail(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(tail) =
            global_indirect_callback_tail(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        crate::move_instruction_before_retargeting(self, tail.start + 3, tail.start);
        crate::move_instruction_before_retargeting(self, tail.start + 2, tail.start + 1);
        crate::move_instruction_before_retargeting(self, tail.start + 3, tail.start + 2);
        crate::move_instruction_before_retargeting(self, tail.start + 4, tail.start + 3);
        // The old receiver copy was also the control-flow block entry. Moving
        // that instruction must not move the entry label past the newly issued
        // pointer load.
        retarget_block_entry(&mut self.output, tail.start + 4, tail.start);

        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[tail.start + 1]
        else {
            unreachable!("the recognized callback high remains a lis");
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } =
            &mut self.output.instructions[tail.start + 2]
        else {
            unreachable!("the recognized callback low remains an addi");
        };
        *a = 3;
        self.output.instructions[tail.start + 4] = Instruction::AddImmediate {
            d: 3,
            a: tail.receiver,
            immediate: 0,
        };
    }
}

fn retarget_block_entry(
    output: &mut mwcc_machine_code::MachineFunction,
    from: usize,
    to: usize,
) {
    for instruction in &mut output.instructions {
        match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } if *target == from => *target = to,
            _ => {}
        }
    }
    for table in &mut output.jump_tables {
        for entry in &mut table.entries {
            if *entry as usize / 4 == from {
                *entry = u32::try_from(to).unwrap_or(u32::MAX).saturating_mul(4);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GlobalIndirectCallbackTail {
    start: usize,
    receiver: u8,
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

fn global_indirect_callback_tail(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<GlobalIndirectCallbackTail> {
    instructions
        .windows(6)
        .enumerate()
        .find_map(|(start, window)| {
            let [
                Instruction::Or {
                    a: 3,
                    s: receiver,
                    b: receiver_again,
                },
                Instruction::AddImmediateShifted {
                    d: callback_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: callback_argument,
                    a: callback_low_base,
                    ..
                },
                Instruction::LoadWord { d: 12, a: 0, .. },
                Instruction::MoveToLinkRegister { s: 12 },
                Instruction::BranchToLinkRegisterAndLink,
            ] = window
            else {
                return None;
            };
            if receiver != receiver_again
                || callback_high != callback_low_base
                || !(3..=10).contains(callback_argument)
            {
                return None;
            }
            let high = relocations.iter().find(|relocation| {
                relocation.instruction_index == start + 1
                    && relocation.kind == RelocationKind::Addr16Ha
            })?;
            let low = relocations.iter().find(|relocation| {
                relocation.instruction_index == start + 2
                    && relocation.kind == RelocationKind::Addr16Lo
            })?;
            if external_target(high) != external_target(low)
                || external_target(high).is_none()
                || !relocations.iter().any(|relocation| {
                    relocation.instruction_index == start + 3
                        && relocation.kind == RelocationKind::EmbSda21
                })
            {
                return None;
            }
            Some(GlobalIndirectCallbackTail {
                start,
                receiver: *receiver,
            })
        })
}

fn legacy_argument_materialization(instruction: &Instruction) -> Option<Instruction> {
    match *instruction {
        Instruction::Or { a, s, b } if s == b && (3..=10).contains(&a) => {
            Some(Instruction::AddImmediate {
                d: a,
                a: s,
                immediate: 0,
            })
        }
        Instruction::AddImmediate { d, .. } if (3..=10).contains(&d) => Some(instruction.clone()),
        _ => None,
    }
}

fn unique_position(
    instructions: &[Instruction],
    predicate: impl Fn(&Instruction) -> bool,
) -> Option<usize> {
    let mut matches = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| predicate(instruction).then_some(index));
    let position = matches.next()?;
    matches.next().is_none().then_some(position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.to_string()),
        }
    }

    #[test]
    fn recognizes_a_global_indirect_callback_tail() {
        let instructions = vec![
            Instruction::move_register(3, 7),
            Instruction::load_immediate_shifted(4, 0),
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];
        let relocations = vec![
            relocation(1, RelocationKind::Addr16Ha, "callback"),
            relocation(2, RelocationKind::Addr16Lo, "callback"),
            relocation(3, RelocationKind::EmbSda21, "callee"),
        ];

        assert_eq!(
            global_indirect_callback_tail(&instructions, &relocations),
            Some(GlobalIndirectCallbackTail {
                start: 0,
                receiver: 7,
            })
        );
    }

    #[test]
    fn keeps_a_reordered_indirect_tail_at_its_control_flow_entry() {
        let mut output = mwcc_machine_code::MachineFunction::new("tail");
        output.instructions = vec![
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 6,
            },
            Instruction::Branch { target: 6 },
        ];

        retarget_block_entry(&mut output, 6, 2);

        assert!(matches!(
            output.instructions[0],
            Instruction::BranchConditionalForward { target: 2, .. }
        ));
        assert!(matches!(
            output.instructions[1],
            Instruction::Branch { target: 2 }
        ));
    }
}
