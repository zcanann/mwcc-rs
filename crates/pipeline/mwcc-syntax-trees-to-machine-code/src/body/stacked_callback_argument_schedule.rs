//! Standard-frame scheduling for a callback beside a stack-local argument.
//!
//! The incoming values first form a two-word stack record.  An O4 direct call
//! then receives a global address, a function address, and that record's
//! address.  MWCC borrows the third argument lane for the callback high half,
//! which lets it fill the LR-save latency slot without destroying incoming r4.

use super::*;

impl Generator {
    pub(crate) fn schedule_stacked_callback_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.schedule_latency_slots
        {
            return;
        }
        let function_symbols = &self.call_return_types;
        schedule_stacked_callback_arguments(&mut self.output, &|name| {
            function_symbols.contains_key(name)
        });
    }
}

fn schedule_stacked_callback_arguments(
    output: &mut mwcc_machine_code::MachineFunction,
    is_function_symbol: &dyn Fn(&str) -> bool,
) {
    let [
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, .. },
        Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: first_slot,
        },
        Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: second_slot,
        },
        Instruction::AddImmediateShifted { d: 3, a: 0, .. },
        Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: callback_high_immediate,
        },
        Instruction::AddImmediate { d: 3, a: 3, .. },
        Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: callback_low_immediate,
        },
        Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: record_slot,
        },
        Instruction::BranchAndLink { .. },
        ..
    ] = output.instructions.as_slice()
    else {
        return;
    };
    if *second_slot != first_slot.saturating_add(4) || record_slot != first_slot {
        return;
    }

    let relocation_name = |index, kind| {
        output.relocations.iter().find_map(|relocation| {
            if relocation.instruction_index != index || relocation.kind != kind {
                return None;
            }
            match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(name) => Some(name.as_str()),
                _ => None,
            }
        })
    };
    let (Some(global_high), Some(global_low), Some(callback_high), Some(callback_low)) = (
        relocation_name(5, RelocationKind::Addr16Ha),
        relocation_name(7, RelocationKind::Addr16Lo),
        relocation_name(6, RelocationKind::Addr16Ha),
        relocation_name(8, RelocationKind::Addr16Lo),
    ) else {
        return;
    };
    if global_high != global_low
        || callback_high != callback_low
        || !is_function_symbol(callback_high)
    {
        return;
    }
    let callback_high_immediate = *callback_high_immediate;
    let callback_low_immediate = *callback_low_immediate;

    crate::permute_machine_function_region(
        output,
        0,
        &[0, 1, 6, 2, 3, 5, 7, 4, 8, 9, 10],
    );
    output.instructions[2] = Instruction::AddImmediateShifted {
        d: 5,
        a: 0,
        immediate: callback_high_immediate,
    };
    output.instructions[8] = Instruction::AddImmediate {
        d: 4,
        a: 5,
        immediate: callback_low_immediate,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn borrows_the_third_lane_for_the_callback_high_half() {
        let mut output = mwcc_machine_code::MachineFunction::new("judge");
        output.instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::StoreWord { s: 3, a: 1, offset: 8 },
            Instruction::StoreWord { s: 4, a: 1, offset: 12 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 8 },
            Instruction::BranchAndLink { target: "iterate".into() },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 5,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("list".into()),
            },
            Relocation {
                instruction_index: 7,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("list".into()),
            },
            Relocation {
                instruction_index: 6,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("filter".into()),
            },
            Relocation {
                instruction_index: 8,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("filter".into()),
            },
        ];

        schedule_stacked_callback_arguments(&mut output, &|name| name == "filter");

        assert!(matches!(output.instructions[2],
            Instruction::AddImmediateShifted { d: 5, a: 0, .. }));
        assert!(matches!(output.instructions[7],
            Instruction::StoreWord { s: 4, a: 1, offset: 12 }));
        assert!(matches!(output.instructions[8],
            Instruction::AddImmediate { d: 4, a: 5, .. }));
        assert_eq!(output.relocations[0].instruction_index, 5);
        assert_eq!(output.relocations[1].instruction_index, 6);
        assert_eq!(output.relocations[2].instruction_index, 2);
        assert_eq!(output.relocations[3].instruction_index, 8);
    }
}
