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
