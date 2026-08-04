//! Prologue issue order for a saved global base and derived member addresses.
//!
//! Structured selection establishes the saved homes and emits their dependency
//! chain after linkage setup. Build 43 fills those independent linkage stores
//! with the global address high half and each derived-address add. This late
//! pass owns only that physical permutation, after allocation has fixed the
//! registers and relocation identities.

#[allow(unused_imports)]
use super::*;

use super::structured_conversion_call_schedule::permute_region;

const ENTRY_SCHEDULE: [usize; 11] = [0, 7, 1, 2, 3, 8, 4, 9, 5, 10, 6];

impl Generator {
    pub(crate) fn schedule_structured_global_base_entry(&mut self) {
        let Some(global) = self
            .structured_global_base_cache
            .as_ref()
            .map(|cache| cache.global.clone())
        else {
            return;
        };
        if retained_global_base_entry(&self.output, &global) {
            permute_region(&mut self.output, 0, &ENTRY_SCHEDULE);
        }
    }

    /// Keep the classic build-163 tail for a saved global base and its one
    /// derived member address.
    ///
    /// Ordinary linkage-first frames restore the stack before writing LR in
    /// this generation. The compact global callback packet instead finishes
    /// its two saved-register reloads, writes LR, and then releases the stack.
    /// Nintendo's derived compiler deliberately uses its stack-first policy
    /// for the same source shape, so the generation discriminator remains part
    /// of this owner rather than becoming a global epilogue exception.
    pub(crate) fn schedule_structured_global_base_epilogue(&mut self) {
        if self.behavior.structured_saved_gpr_stack_first
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterAfterStackRestore
        {
            return;
        }
        let Some((stack_restore, link_restore)) =
            structured_global_base_epilogue_packet(&self.output, self.frame_size)
        else {
            return;
        };
        crate::move_instruction_before_retargeting(self, link_restore, stack_restore);
    }
}

fn structured_global_base_epilogue_packet(
    output: &mwcc_machine_code::MachineFunction,
    frame_size: i16,
) -> Option<(usize, usize)> {
    let instructions = &output.instructions;
    if !matches!(
        instructions.get(..8),
        Some([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
            Instruction::StoreWord { s: 31, a: 1, .. },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 30, a: 1, .. },
            Instruction::AddImmediate { d: 30, a: 31, immediate },
        ]) if *offset == -frame_size && *immediate != 0
    ) || !matches!(
        (
            external_target(output, 1, RelocationKind::Addr16Ha),
            external_target(output, 5, RelocationKind::Addr16Lo),
        ),
        (Some(high), Some(low)) if high == low
    )
    {
        return None;
    }
    let tail = instructions.len().checked_sub(5)?;
    matches!(
        &instructions[tail..],
        [
            Instruction::LoadWord { d: 31, a: 1, .. },
            Instruction::LoadWord { d: 30, a: 1, .. },
            Instruction::AddImmediate { d: 1, a: 1, immediate },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ] if *immediate == frame_size
    )
    .then_some((tail + 2, tail + 3))
}

fn retained_global_base_entry(output: &mwcc_machine_code::MachineFunction, global: &str) -> bool {
    let instructions = &output.instructions;
    let Some(Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: frame_push,
    }) = instructions.get(2)
    else {
        return false;
    };
    let frame_size = -*frame_push;
    instructions.len() >= ENTRY_SCHEDULE.len()
        && frame_size >= 16
        && external_target(output, 7, RelocationKind::Addr16Ha) == Some(global)
        && external_target(output, 8, RelocationKind::Addr16Lo) == Some(global)
        && matches!(
            instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: 31, a: 1, offset: first },
                Instruction::StoreWord { s: 30, a: 1, offset: second },
                Instruction::StoreWord { s: 29, a: 1, offset: third },
                Instruction::StoreWord { s: 28, a: 1, offset: fourth },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
                Instruction::AddImmediate { d: 30, a: 31, .. },
                Instruction::AddImmediate { d: 29, a: 31, .. },
                ..
            ] if *first == frame_size - 4
                && *second == frame_size - 8
                && *third == frame_size - 12
                && *fourth == frame_size - 16
        )
}

fn external_target<'a>(
    output: &'a mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&'a str> {
    output.relocations.iter().find_map(|relocation| {
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
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External("ActivePlayer".into()),
        }
    }

    #[test]
    fn interleaves_a_saved_global_base_with_linkage_stores() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::StoreWord { s: 30, a: 1, offset: 24 },
            Instruction::StoreWord { s: 29, a: 1, offset: 20 },
            Instruction::StoreWord { s: 28, a: 1, offset: 16 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 30, a: 31, immediate: 216 },
            Instruction::AddImmediate { d: 29, a: 31, immediate: 167 },
        ];
        output.relocations = vec![
            relocation(7, RelocationKind::Addr16Ha),
            relocation(8, RelocationKind::Addr16Lo),
        ];

        assert!(retained_global_base_entry(&output, "ActivePlayer"));
        permute_region(&mut output, 0, &ENTRY_SCHEDULE);

        assert!(matches!(output.instructions[1], Instruction::AddImmediateShifted { .. }));
        assert!(matches!(output.instructions[5], Instruction::AddImmediate { d: 31, .. }));
        assert!(matches!(output.instructions[7], Instruction::AddImmediate { d: 30, .. }));
        assert!(matches!(output.instructions[9], Instruction::AddImmediate { d: 29, .. }));
        assert_eq!(output.relocations[0].instruction_index, 1);
        assert_eq!(output.relocations[1].instruction_index, 5);
    }

    #[test]
    fn recognizes_the_stack_first_compact_global_callback_tail() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 30, a: 31, immediate: 64 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations = vec![
            relocation(1, RelocationKind::Addr16Ha),
            relocation(5, RelocationKind::Addr16Lo),
        ];

        assert_eq!(
            structured_global_base_epilogue_packet(&output, 24),
            Some((11, 12))
        );
    }

    #[test]
    fn rejects_a_tail_with_more_than_the_compact_saved_pair() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 30, a: 31, immediate: 64 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::LoadWord { d: 29, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations = vec![
            relocation(1, RelocationKind::Addr16Ha),
            relocation(5, RelocationKind::Addr16Lo),
        ];

        assert_eq!(structured_global_base_epilogue_packet(&output, 24), None);
    }
}
