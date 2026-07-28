//! Build 163 call scheduling for incoming parameters with escaped frame addresses.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Schedule calls that publish an incoming parameter through its frame
    /// address. Build 163 finishes independent register arguments before the
    /// spill, forms all frame addresses before chasing a pointer load chain,
    /// and puts an independent frame reload ahead of a first-argument mask on
    /// the later consumer call.
    pub(crate) fn schedule_linkage_first_addressable_parameter_calls(&mut self) {
        let parameter_slots: Vec<_> = self
            .frame_slots
            .values()
            .filter_map(|slot| slot.parameter_register.map(|register| (slot.offset, register)))
            .collect();
        if parameter_slots.is_empty() {
            return;
        }
        let frame_offsets: Vec<_> = self.frame_slots.values().map(|slot| slot.offset).collect();
        schedule_publication(
            &mut self.output,
            &mut self.labels,
            &parameter_slots,
            &frame_offsets,
        );
        schedule_reload(&mut self.output, &mut self.labels, &parameter_slots);
    }
}

fn move_instruction_before(
    output: &mut mwcc_machine_code::MachineFunction,
    labels: &mut mwcc_vreg::Labels,
    from: usize,
    to: usize,
) {
    debug_assert!(to < from);
    let instruction = output.instructions.remove(from);
    output.instructions.insert(to, instruction);
    labels.moved_before(from, to);
    for relocation in &mut output.relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == from => to,
            index if (to..from).contains(&index) => index + 1,
            index => index,
        };
    }
}

fn schedule_publication(
    output: &mut mwcc_machine_code::MachineFunction,
    labels: &mut mwcc_vreg::Labels,
    parameter_slots: &[(i16, u8)],
    frame_offsets: &[i16],
) {
    let Some(first_call) = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { .. })
    }) else {
        return;
    };
    let Some(spill) = output.instructions[..first_call]
        .iter()
        .enumerate()
        .find_map(|(index, instruction)| match instruction {
            Instruction::StoreWord { s, a: 1, offset }
                if parameter_slots.contains(&(*offset, *s)) =>
            {
                Some(index)
            }
            _ => None,
        })
    else {
        return;
    };
    let Some(Instruction::AddImmediate {
        d,
        a,
        immediate: 0,
    }) = output.instructions.get(spill + 1)
    else {
        return;
    };
    if !(3..=10).contains(d) || !(3..=10).contains(a) {
        return;
    }
    if output.relocations.iter().any(|relocation| {
        (spill..first_call).contains(&relocation.instruction_index)
    }) {
        return;
    }

    // Register-to-register argument setup precedes publication of the incoming
    // frame object.
    move_instruction_before(output, labels, spill + 1, spill);
    let spill = spill + 1;
    let mut insertion = spill + 1;
    loop {
        let Some(address) = (insertion..first_call).find(|&index| {
            matches!(
                output.instructions[index],
                Instruction::AddImmediate {
                    d: 3..=10,
                    a: 1,
                    immediate,
                } if frame_offsets.contains(&immediate)
            )
        }) else {
            break;
        };
        if address == insertion {
            insertion += 1;
            continue;
        }
        move_instruction_before(output, labels, address, insertion);
        insertion += 1;
    }
}

fn schedule_reload(
    output: &mut mwcc_machine_code::MachineFunction,
    labels: &mut mwcc_vreg::Labels,
    parameter_slots: &[(i16, u8)],
) {
    let calls: Vec<_> = output
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(instruction, Instruction::BranchAndLink { .. }).then_some(index)
        })
        .collect();
    for call in calls.into_iter().skip(1) {
        if call < 3 {
            continue;
        }
        let mask = &output.instructions[call - 3];
        let independent_reload = &output.instructions[call - 2];
        let parameter_reload = &output.instructions[call - 1];
        let mask_defines_first_argument = mwcc_vreg::register_operands(mask)
            .into_iter()
            .any(|operand| {
                operand.class == mwcc_vreg::Class::General
                    && operand.role == mwcc_vreg::RegisterRole::Define
                    && operand.register == 3
            });
        if !mask_defines_first_argument
            || !matches!(
                independent_reload,
                Instruction::LoadWord { d: 4, a: 1, .. }
            )
            || !matches!(
                parameter_reload,
                Instruction::LoadWord { d: 5, a: 1, offset }
                    if parameter_slots.iter().any(|(slot, _)| slot == offset)
            )
            || touches_register(mask, 4)
            || touches_register(independent_reload, 3)
            || output.relocations.iter().any(|relocation| {
                (call - 3..call).contains(&relocation.instruction_index)
            })
        {
            continue;
        }
        move_instruction_before(output, labels, call - 2, call - 3);
    }
}

fn touches_register(instruction: &Instruction, register: u8) -> bool {
    mwcc_vreg::register_operands(instruction)
        .into_iter()
        .any(|operand| {
            operand.class == mwcc_vreg::Class::General && operand.register == register
        })
}
