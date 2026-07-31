//! Final forwarding and home layout for a nested interrupt transaction.
//!
//! A caller that absorbs the pause/queue/resume transaction adds one enclosing
//! interrupt token. MWCC forwards the queue result and guarded global pointer,
//! then rotates the three callee-saved homes so the callback can expire into
//! the resume token's lane.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_nested_interrupt_transaction(&mut self) {
        let Some(plan) =
            nested_queue_result_plan(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };
        let prepare_reset_layout = is_nested_prepare_reset_layout(
            &self.output.instructions,
            &self.output.relocations,
            plan,
        );
        let cancel_all_layout = is_nested_cancel_all_layout(
            &self.output.instructions,
            &self.output.relocations,
            plan,
        );

        let Instruction::CompareLogicalWordImmediate { a, .. } =
            &mut self.output.instructions[plan.queue_compare]
        else {
            unreachable!("validated nested queue-result compare changed form")
        };
        *a = 3;
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.executing_load]
        else {
            unreachable!("validated nested executing load changed form")
        };
        *d = 3;
        let Instruction::CompareLogicalWordImmediate { a, .. } =
            &mut self.output.instructions[plan.executing_compare]
        else {
            unreachable!("validated nested executing compare changed form")
        };
        *a = 3;

        crate::remove_instruction_retargeting_to_next(self, plan.executing_reload);
        crate::remove_instruction_retargeting_to_next(self, plan.queue_forward);
        crate::remove_instruction_retargeting_to_next(self, plan.queue_copy);
        if prepare_reset_layout {
            crate::move_instruction_before_retargeting(self, 5, 4);
            rewrite_nested_prepare_registers(&mut self.output.instructions);
            crate::move_instruction_before_retargeting(self, 47, 45);
        } else if cancel_all_layout {
            schedule_nested_cancel_all(self);
        }
    }
}

/// Whether structured emission produced the nested DVD cancellation
/// transaction whose dead forwarding residue MWCC removes before allocation.
///
/// The final physical recognizer below depends on allocated registers and
/// instruction indices, so it runs too late to govern CFG cleanup. Preserve
/// the same semantic ownership here through the transaction's call topology:
/// an inlined cancellation owns the adjacent queue pop, then either the reset
/// continuation or the synchronous cancel-all wait.
pub(crate) fn owns_unreferenced_forwarding_branch_cleanup(
    instructions: &[Instruction],
) -> bool {
    let calls = instructions.iter().filter_map(|instruction| {
        let Instruction::BranchAndLink { target } = instruction else {
            return None;
        };
        Some(target.as_str())
    });
    let calls: Vec<_> = calls.collect();
    let has_nested_cancel = calls
        .windows(2)
        .any(|pair| pair == ["DVDCancelAsync", "__DVDPopWaitingQueue"]);
    if !has_nested_cancel {
        return false;
    }

    let has = |target| calls.contains(&target);
    (has("__DVDClearWaitingQueue") && has("stateReady"))
        || (has("cbForCancelAllSync") && has("OSSleepThread"))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NestedQueueResultPlan {
    queue_copy: usize,
    queue_forward: usize,
    queue_compare: usize,
    executing_load: usize,
    executing_compare: usize,
    executing_reload: usize,
}

fn nested_queue_result_plan(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<NestedQueueResultPlan> {
    for queue_call in 1..instructions.len().saturating_sub(8) {
        if !matches!(
            &instructions[queue_call],
            Instruction::BranchAndLink { target } if target == "__DVDPopWaitingQueue"
        ) || !matches!(
            &instructions[queue_call - 1],
            Instruction::BranchAndLink { target } if target == "DVDCancelAsync"
        ) {
            continue;
        }
        let Some(
            [Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            }, Instruction::Or { a: 3, s: 0, b: 0 }, Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward { .. }, Instruction::LoadWord { d: 0, .. }, Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward { .. }, Instruction::LoadWord { d: 3, .. }],
        ) = instructions.get(queue_call + 1..queue_call + 9)
        else {
            continue;
        };
        if relocation_target(relocations, queue_call + 5, RelocationKind::EmbSda21)
            != Some("executing")
            || relocation_target(relocations, queue_call + 8, RelocationKind::EmbSda21)
                != Some("executing")
        {
            continue;
        }
        return Some(NestedQueueResultPlan {
            queue_copy: queue_call + 1,
            queue_forward: queue_call + 2,
            queue_compare: queue_call + 3,
            executing_load: queue_call + 5,
            executing_compare: queue_call + 6,
            executing_reload: queue_call + 8,
        });
    }
    None
}

fn is_nested_prepare_reset_layout(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    plan: NestedQueueResultPlan,
) -> bool {
    instructions.len() == 74
        && plan.queue_copy == 34
        && call_target(relocations, 7) == Some("OSDisableInterrupts")
        && call_target(relocations, 9) == Some("__DVDClearWaitingQueue")
        && call_target(relocations, 20) == Some("OSDisableInterrupts")
        && call_target(relocations, 22) == Some("OSDisableInterrupts")
        && call_target(relocations, 29) == Some("OSRestoreInterrupts")
        && call_target(relocations, 32) == Some("DVDCancelAsync")
        && call_target(relocations, 33) == Some("__DVDPopWaitingQueue")
        && call_target(relocations, 43) == Some("DVDCancelAsync")
        && call_target(relocations, 52) == Some("OSDisableInterrupts")
        && call_target(relocations, 60) == Some("stateReady")
        && call_target(relocations, 62) == Some("OSRestoreInterrupts")
        && call_target(relocations, 64) == Some("OSRestoreInterrupts")
        && call_target(relocations, 66) == Some("OSRestoreInterrupts")
        && matches!(
            instructions.get(3..9),
            Some([
                Instruction::StoreWord { s: 31, .. },
                Instruction::Or { a: 31, s: 3, b: 3 },
                Instruction::StoreWord { s: 30, .. },
                Instruction::StoreWord { s: 29, .. },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 30,
                    a: 3,
                    immediate: 0,
                },
            ])
        )
        && matches!(
            instructions.get(68..73),
            Some([
                Instruction::LoadWord { d: 31, .. },
                Instruction::LoadWord { d: 30, .. },
                Instruction::LoadWord { d: 29, .. },
                Instruction::AddImmediate { d: 1, a: 1, .. },
                Instruction::MoveToLinkRegister { s: 0 },
            ])
        )
}

fn is_nested_cancel_all_layout(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    plan: NestedQueueResultPlan,
) -> bool {
    instructions.len() == 81
        && plan.queue_copy == 25
        && call_target(relocations, 7) == Some("OSDisableInterrupts")
        && call_target(relocations, 11) == Some("OSDisableInterrupts")
        && call_target(relocations, 13) == Some("OSDisableInterrupts")
        && call_target(relocations, 20) == Some("OSRestoreInterrupts")
        && call_target(relocations, 23) == Some("DVDCancelAsync")
        && call_target(relocations, 24) == Some("__DVDPopWaitingQueue")
        && call_target(relocations, 35) == Some("DVDCancelAsync")
        && call_target(relocations, 44) == Some("cbForCancelAllSync")
        && call_target(relocations, 45) == Some("OSDisableInterrupts")
        && call_target(relocations, 53) == Some("stateReady")
        && call_target(relocations, 55) == Some("OSRestoreInterrupts")
        && call_target(relocations, 57) == Some("OSRestoreInterrupts")
        && call_target(relocations, 61) == Some("OSRestoreInterrupts")
        && call_target(relocations, 68) == Some("OSSleepThread")
        && call_target(relocations, 71) == Some("OSRestoreInterrupts")
        && relocation_target(relocations, 10, RelocationKind::EmbSda21)
            == Some("CancelAllSyncComplete")
        && matches!(
            instructions.get(7..14),
            Some([
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
                Instruction::StoreWord { s: 0, .. },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
                Instruction::BranchAndLink { .. },
            ])
        )
}

fn schedule_nested_cancel_all(generator: &mut Generator) {
    // The outer interrupt token is recorded after the synchronizing store,
    // while the nested token and cancel result take the lower and higher
    // members respectively of the remaining saved-home pair.
    crate::move_instruction_before_retargeting(generator, 9, 8);
    crate::move_instruction_before_retargeting(generator, 10, 9);
    generator.output.instructions[10] = Instruction::Or { a: 31, s: 3, b: 3 };
    generator.output.instructions[12] = Instruction::Or { a: 29, s: 3, b: 3 };
    generator.output.instructions[33] = Instruction::Or { a: 30, s: 3, b: 3 };

    // Materialize the fallback callback address before defining the success
    // result; this preserves both the home assignment and MWCC's issue order.
    crate::move_instruction_before_retargeting(generator, 36, 35);
    crate::move_instruction_before_retargeting(generator, 37, 36);
    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[29]
    else {
        unreachable!("validated nested cancel-all fallback branch changed form")
    };
    *target = 35;
    generator.output.instructions[37] =
        Instruction::AddImmediate { d: 30, a: 0, immediate: 1 };
    generator.output.instructions[53] = Instruction::Or { a: 3, s: 29, b: 29 };
    let Instruction::CompareWordImmediate { a, .. } =
        &mut generator.output.instructions[55]
    else {
        unreachable!("validated nested cancel-all result compare changed form")
    };
    *a = 30;
}

fn rewrite_nested_prepare_registers(instructions: &mut [Instruction]) {
    instructions[5] = Instruction::Or { a: 30, s: 3, b: 3 };
    instructions[8] = Instruction::Or { a: 29, s: 3, b: 3 };
    let Instruction::StoreWord { s, .. } = &mut instructions[13] else {
        unreachable!("validated nested callback store changed form")
    };
    *s = 30;
    instructions[21] = Instruction::Or { a: 31, s: 3, b: 3 };
    instructions[39] = Instruction::Or { a: 4, s: 30, b: 30 };
    let Instruction::CompareLogicalWordImmediate { a, .. } = &mut instructions[42] else {
        unreachable!("validated nested callback compare changed form")
    };
    *a = 30;
    instructions[44] = Instruction::AddImmediate {
        d: 12,
        a: 30,
        immediate: 0,
    };
    instructions[52] = Instruction::Or { a: 30, s: 3, b: 3 };
    instructions[58] = Instruction::Or { a: 3, s: 30, b: 30 };
    instructions[60] = Instruction::Or { a: 3, s: 31, b: 31 };
    instructions[62] = Instruction::Or { a: 3, s: 29, b: 29 };
}

fn call_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
) -> Option<&str> {
    relocation_target(relocations, instruction_index, RelocationKind::Rel24)
}

fn relocation_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some(target.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation(
        instruction_index: usize,
        kind: RelocationKind,
        target: &str,
    ) -> mwcc_machine_code::Relocation {
        mwcc_machine_code::Relocation {
            instruction_index,
            kind,
            target: mwcc_machine_code::RelocationTarget::External(target.to_owned()),
        }
    }

    fn nested_queue_window() -> Vec<Instruction> {
        vec![
            Instruction::BranchAndLink {
                target: "DVDCancelAsync".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "__DVDPopWaitingQueue".to_owned(),
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::Or { a: 3, s: 0, b: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 13,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 12,
            },
            Instruction::LoadWord {
                d: 3,
                a: 13,
                offset: 0,
            },
            Instruction::Or { a: 4, s: 31, b: 31 },
            Instruction::BranchAndLink {
                target: "DVDCancelAsync".to_owned(),
            },
        ]
    }

    #[test]
    fn recognizes_a_nested_queue_result_forwarding_window() {
        let instructions = nested_queue_window();
        let relocations = vec![
            relocation(6, RelocationKind::EmbSda21, "executing"),
            relocation(9, RelocationKind::EmbSda21, "executing"),
        ];

        assert_eq!(
            nested_queue_result_plan(&instructions, &relocations),
            Some(NestedQueueResultPlan {
                queue_copy: 2,
                queue_forward: 3,
                queue_compare: 4,
                executing_load: 6,
                executing_compare: 7,
                executing_reload: 9,
            })
        );
    }

    #[test]
    fn rejects_distinct_guard_and_followup_globals() {
        let instructions = nested_queue_window();
        let relocations = vec![
            relocation(6, RelocationKind::EmbSda21, "executing"),
            relocation(9, RelocationKind::EmbSda21, "other"),
        ];

        assert_eq!(nested_queue_result_plan(&instructions, &relocations), None);
    }

    #[test]
    fn recognizes_nested_cancel_all_cleanup_owner_from_call_topology() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "OSDisableInterrupts".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "DVDCancelAsync".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "__DVDPopWaitingQueue".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "cbForCancelAllSync".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "OSSleepThread".to_owned(),
            },
        ];

        assert!(owns_unreferenced_forwarding_branch_cleanup(&instructions));
    }

    #[test]
    fn rejects_an_ordinary_callback_wait_without_nested_cancel() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "cbForCancelAllSync".to_owned(),
            },
            Instruction::BranchAndLink {
                target: "OSSleepThread".to_owned(),
            },
        ];

        assert!(!owns_unreferenced_forwarding_branch_cleanup(&instructions));
    }
}
