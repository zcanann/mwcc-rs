//! Final issue order for a state-machine read entry.
//!
//! Build 163 overlaps independent callback/global address formation with the
//! linkage prologue, assertion packet, and final asynchronous-read arguments.
//! Selection and allocation retain the right values; this late owner applies
//! the physical issue order once registers and relocations are final.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StateReadEntrySchedule {
    panic_call: usize,
    read_call: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_state_read_entry(&mut self) {
        let diagnostic = std::env::var_os("MWCC_DIAGNOSTIC_STATE_READ")
            .is_some_and(|requested| {
                requested == "*" || requested == std::ffi::OsStr::new(&self.output.name)
            });
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let plan = state_read_entry_schedule(
            &self.output.instructions,
            &self.output.relocations,
        );
        if diagnostic {
            eprintln!(
                "state read {} plan={plan:?} instructions={:?} relocations={:?}",
                self.output.name, self.output.instructions, self.output.relocations
            );
        }
        let Some(plan) = plan else {
            return;
        };

        // Hoist the published state address through the linkage prologue and
        // interleave the independent BB2 base with the frame setup.
        crate::move_instruction_before_retargeting(self, 7, 1);
        crate::move_instruction_before_retargeting(self, 8, 3);
        crate::move_instruction_before_retargeting(self, 6, 4);
        crate::move_instruction_before_retargeting(self, 7, 6);

        crate::move_instruction_before_retargeting(self, 10, 9);
        crate::move_instruction_before_retargeting(self, 12, 11);
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[10] else {
            unreachable!("validated state-read boot receiver changed form")
        };
        *d = 4;
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[12] else {
            unreachable!("validated state-read assertion member changed form")
        };
        *a = 4;

        // The variadic CR declaration is issued as soon as the format high
        // half is available, before the low half and fixed arguments.
        crate::move_instruction_before_retargeting(self, plan.panic_call - 1, 16);

        // Form all four DVDLowRead arguments as one interleaved transaction.
        crate::move_instruction_before_retargeting(self, 26, 21);
        crate::move_instruction_before_retargeting(self, 24, 22);
        crate::move_instruction_before_retargeting(self, 27, 23);
        crate::move_instruction_before_retargeting(self, 29, 25);
        crate::move_instruction_before_retargeting(self, 29, 26);
        crate::move_instruction_before_retargeting(self, 28, 27);
        crate::move_instruction_before_retargeting(self, 30, 29);

        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[14]
        else {
            unreachable!("validated state-read assertion branch changed form")
        };
        *target = 21;

        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[21]
        else {
            unreachable!("validated state-read BB2 high half changed form")
        };
        *d = 3;
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[22] else {
            unreachable!("validated state-read length load changed form")
        };
        *d = 6;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[23] else {
            unreachable!("validated state-read BB2 low half changed form")
        };
        *a = 3;
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[24] else {
            unreachable!("validated state-read boot load changed form")
        };
        *d = 7;
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[25]
        else {
            unreachable!("validated state-read callback high half changed form")
        };
        *d = 4;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[27] else {
            unreachable!("validated state-read rounded length changed form")
        };
        *a = 6;
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[28] else {
            unreachable!("validated state-read source load changed form")
        };
        *a = 7;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[29] else {
            unreachable!("validated state-read callback low half changed form")
        };
        *a = 4;

        debug_assert_eq!(plan.read_call, 31);
    }
}

fn state_read_entry_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<StateReadEntrySchedule> {
    let panic_call = relocation_index(relocations, RelocationKind::Rel24, "OSPanic")?;
    let read_call = relocation_index(relocations, RelocationKind::Rel24, "DVDLowRead")?;
    if panic_call != 20
        || read_call != 31
        || !matches!(
            instructions.get(..read_call + 1),
            Some([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
                Instruction::StoreWord { s: 31, a: 1, offset: 12 },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::AddImmediate { d: 31, a: 3, immediate: 8 },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 0, a: 3, .. },
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::StoreWord { s: 0, a: 0, offset: 0 },
                Instruction::LoadWord { d: 3, a: 3, offset: 60 },
                Instruction::LoadWord { d: 0, a: 31, offset: 0 },
                Instruction::CompareLogicalWord { a: 3, b: 0 },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 0,
                    target: 21,
                },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 3, .. },
                Instruction::AddImmediate { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 4, a: 0, immediate: 661 },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { .. },
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::LoadWord { d: 3, a: 3, offset: 56 },
                Instruction::LoadWord { d: 4, a: 31, offset: 0 },
                Instruction::AddImmediate { d: 0, a: 4, immediate: 31 },
                Instruction::AndContiguousMask {
                    a: 4,
                    s: 0,
                    begin: 0,
                    end: 26,
                },
                Instruction::AddImmediateShifted { d: 5, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 5, .. },
                Instruction::LoadWord { d: 5, a: 5, offset: 4 },
                Instruction::AddImmediateShifted { d: 6, a: 0, .. },
                Instruction::AddImmediate { d: 6, a: 6, .. },
                Instruction::BranchAndLink { .. },
            ])
        )
    {
        return None;
    }

    let state = relocation_target_at(relocations, 7, RelocationKind::Addr16Ha)?;
    let bb2 = relocation_target_at(relocations, 4, RelocationKind::Addr16Ha)?;
    let callback = relocation_target_at(relocations, 29, RelocationKind::Addr16Ha)?;
    if relocation_target_at(relocations, 8, RelocationKind::Addr16Lo) != Some(state)
        || relocation_target_at(relocations, 10, RelocationKind::EmbSda21) != Some("LastState")
        || relocation_target_at(relocations, 9, RelocationKind::EmbSda21) != Some("bootInfo")
        || relocation_target_at(relocations, 15, RelocationKind::Addr16Ha).is_none()
        || relocation_target_at(relocations, 16, RelocationKind::Addr16Lo).is_none()
        || relocation_target_at(relocations, 17, RelocationKind::EmbSda21).is_none()
        || relocation_target_at(relocations, 21, RelocationKind::EmbSda21) != Some("bootInfo")
        || relocation_target_at(relocations, 5, RelocationKind::Addr16Lo) != Some(bb2)
        || relocation_target_at(relocations, 26, RelocationKind::Addr16Ha) != Some(bb2)
        || relocation_target_at(relocations, 27, RelocationKind::Addr16Lo) != Some(bb2)
        || relocation_target_at(relocations, 30, RelocationKind::Addr16Lo) != Some(callback)
    {
        return None;
    }
    Some(StateReadEntrySchedule {
        panic_call,
        read_call,
    })
}

fn relocation_index(
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

fn relocation_target_at(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
            .then(|| match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(name) => Some(name.as_str()),
                _ => None,
            })
            .flatten()
    })
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
    fn recognizes_a_state_read_entry_packet() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 8 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::StoreWord { s: 0, a: 0, offset: 0 },
            Instruction::LoadWord { d: 3, a: 3, offset: 60 },
            Instruction::LoadWord { d: 0, a: 31, offset: 0 },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 21 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 661 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "OSPanic".into() },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadWord { d: 3, a: 3, offset: 56 },
            Instruction::LoadWord { d: 4, a: 31, offset: 0 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 31 },
            Instruction::AndContiguousMask { a: 4, s: 0, begin: 0, end: 26 },
            Instruction::AddImmediateShifted { d: 5, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 5, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 5, offset: 4 },
            Instruction::AddImmediateShifted { d: 6, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 6, a: 6, immediate: 0 },
            Instruction::BranchAndLink { target: "DVDLowRead".into() },
        ];
        let relocations = vec![
            relocation(4, RelocationKind::Addr16Ha, "BB2"),
            relocation(5, RelocationKind::Addr16Lo, "BB2"),
            relocation(7, RelocationKind::Addr16Ha, "stateReadingFST"),
            relocation(8, RelocationKind::Addr16Lo, "stateReadingFST"),
            relocation(9, RelocationKind::EmbSda21, "bootInfo"),
            relocation(10, RelocationKind::EmbSda21, "LastState"),
            relocation(15, RelocationKind::Addr16Ha, "message"),
            relocation(16, RelocationKind::Addr16Lo, "message"),
            relocation(17, RelocationKind::EmbSda21, "source"),
            relocation(20, RelocationKind::Rel24, "OSPanic"),
            relocation(21, RelocationKind::EmbSda21, "bootInfo"),
            relocation(26, RelocationKind::Addr16Ha, "BB2"),
            relocation(27, RelocationKind::Addr16Lo, "BB2"),
            relocation(29, RelocationKind::Addr16Ha, "cbForStateReadingFST"),
            relocation(30, RelocationKind::Addr16Lo, "cbForStateReadingFST"),
            relocation(31, RelocationKind::Rel24, "DVDLowRead"),
        ];

        assert_eq!(
            state_read_entry_schedule(&instructions, &relocations),
            Some(StateReadEntrySchedule { panic_call: 20, read_call: 31 })
        );
    }
}
