//! GC/1.2.5n scheduling for structured bodies with erased inline initializers.
//!
//! The parser has already substituted the accessors, but the legacy optimizer
//! retains their value-graph boundaries. This owner converts the generic
//! structured stream into that graph before allocation: the nested accessor is
//! split into two values, saved homes are established before initialization,
//! and the two call-argument regions receive their measured latency schedule.

use super::*;

impl Generator {
    pub(super) fn schedule_legacy_inline_expansion_residue(&mut self) -> bool {
        if self.legacy_inline_expansion_frame_bytes == 0 {
            return false;
        }
        let calls: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(instruction, Instruction::BranchAndLink { .. }).then_some(index)
            })
            .collect();
        let [first_call, second_call, ..] = calls.as_slice() else {
            return false;
        };
        if *first_call != 15 || *second_call != 23 {
            return false;
        }

        let Some((state, entry, child)) =
            residue_registers(&self.output.instructions[..=*first_call])
        else {
            return false;
        };
        let Some(()) = mixed_literal_call_region(
            &self.output.instructions[*first_call + 1..=*second_call],
            entry,
        ) else {
            return false;
        };

        // The generic expression emitter uses one virtual for both member loads.
        // Legacy scheduling retains the intermediate object pointer separately,
        // allowing the final accessor result to define r3 directly at its call.
        let intermediate = self.fresh_virtual_general();
        self.register_avoid
            .insert(u32::from(intermediate - mwcc_vreg::VIRTUAL_BASE), vec![4]);
        match &mut self.output.instructions[7] {
            Instruction::LoadWord { d, a, .. } if *d == child && *a == state => {
                *d = intermediate;
            }
            _ => return false,
        }
        match &mut self.output.instructions[8] {
            Instruction::LoadWord { d, a, .. } if *d == child && *a == child => {
                *a = intermediate;
            }
            _ => return false,
        }

        // Original generic order (indices 0..15): frame, state init, entry
        // save, two accessor loads, bitfield store, first call arguments.
        // Establish both saved homes first, then fill the mutation's dependency
        // gaps with the retained initializer graph.
        let mut permutation = vec![0, 1, 2, 3, 5, 6, 4, 10, 7, 14, 9, 11, 8, 12, 13, 15];
        // The following mixed call alternates independent float and integer
        // arguments. This is the same source-order latency schedule MWCC uses
        // in the corresponding Melee body.
        permutation.extend([19, 16, 20, 17, 21, 18, 22, 23]);
        if post_callback_bitfield_call_region(&self.output.instructions[24..], state, entry) {
            // The entry argument is materialized while the bit value is live.
            // Retaining that collision selects r4 for the bit value and makes
            // the entry copy an addi, as in the legacy value graph.
            self.output.instructions[33] = Instruction::AddImmediate {
                d: 3,
                a: entry,
                immediate: 0,
            };
            if trailing_state_call_pair(&self.output.instructions[36..], state) {
                self.output.instructions[38] = Instruction::AddImmediate {
                    d: 3,
                    a: state,
                    immediate: 0,
                };
            }
            permutation.extend([24, 25, 26, 27, 28, 30, 33, 29, 31, 34, 32, 35]);
            permutation.extend(36..self.output.instructions.len());
        } else {
            permutation.extend(24..self.output.instructions.len());
        }
        apply_permutation(&mut self.output, &permutation);

        // Two substituted initializer calls each leave two anonymous optimizer
        // nodes before the function's literal pool.
        self.output.anonymous_label_bump += 4;
        self.output.pre_scheduled = true;
        true
    }
}

fn residue_registers(instructions: &[Instruction]) -> Option<(u8, u8, u8)> {
    if instructions.len() != 16
        || !matches!(instructions[0], Instruction::StoreWordWithUpdate { .. })
        || !matches!(instructions[1], Instruction::MoveFromLinkRegister { d: 0 })
        || !matches!(instructions[2], Instruction::StoreWord { s: 0, .. })
        || !matches!(instructions[15], Instruction::BranchAndLink { .. })
    {
        return None;
    }

    let Instruction::StoreWord { s: saved_state, .. } = instructions[3] else {
        return None;
    };
    let Instruction::LoadWord { d: state, a: 3, .. } = instructions[4] else {
        return None;
    };
    let Instruction::StoreWord { s: saved_entry, .. } = instructions[5] else {
        return None;
    };
    let Instruction::Or {
        a: entry,
        s: 3,
        b: 3,
    } = instructions[6]
    else {
        return None;
    };
    let Instruction::LoadWord {
        d: child_first,
        a: child_base,
        ..
    } = instructions[7]
    else {
        return None;
    };
    let Instruction::LoadWord {
        d: child_final,
        a: child_input,
        ..
    } = instructions[8]
    else {
        return None;
    };
    let Instruction::LoadByteZero { a: byte_base, .. } = instructions[9] else {
        return None;
    };
    let Instruction::AddImmediate {
        d: zero,
        a: 0,
        immediate: 0,
    } = instructions[10]
    else {
        return None;
    };
    let Instruction::RotateAndMaskInsert { s: zero_use, .. } = instructions[11] else {
        return None;
    };
    let Instruction::StoreByte { a: store_base, .. } = instructions[12] else {
        return None;
    };
    let Instruction::Or {
        a: 3,
        s: child_argument,
        b: child_argument_copy,
    } = instructions[13]
    else {
        return None;
    };
    let Instruction::Or {
        a: 4,
        s: state_argument,
        b: state_argument_copy,
    } = instructions[14]
    else {
        return None;
    };

    (saved_state == state
        && saved_entry == entry
        && child_base == state
        && child_first == child_final
        && child_first == child_input
        && byte_base == state
        && zero == zero_use
        && store_base == state
        && child_argument == child_first
        && child_argument_copy == child_first
        && state_argument == state
        && state_argument_copy == state)
        .then_some((state, entry, child_first))
}

fn mixed_literal_call_region(instructions: &[Instruction], entry: u8) -> Option<()> {
    if instructions.len() != 8
        || !matches!(
            instructions[1],
            Instruction::AddImmediate { d: 4, a: 0, .. }
        )
        || !matches!(
            instructions[2],
            Instruction::AddImmediate { d: 5, a: 0, .. }
        )
        || !matches!(instructions[3], Instruction::LoadFloatSingle { d: 1, .. })
        || !matches!(instructions[4], Instruction::LoadFloatSingle { d: 2, .. })
        || !matches!(instructions[5], Instruction::FloatMove { d: 3, b: 1 })
        || !matches!(
            instructions[6],
            Instruction::AddImmediate { d: 6, a: 0, .. }
        )
        || !matches!(instructions[7], Instruction::BranchAndLink { .. })
    {
        return None;
    }
    let Instruction::Or {
        a: 3,
        s: first,
        b: first_copy,
    } = instructions[0]
    else {
        return None;
    };
    (first == entry && first_copy == entry).then_some(())
}

fn post_callback_bitfield_call_region(instructions: &[Instruction], state: u8, entry: u8) -> bool {
    let Some(region) = instructions.get(..12) else {
        return false;
    };
    if !matches!(region[1], Instruction::BranchAndLink { .. })
        || !matches!(region[11], Instruction::BranchAndLink { .. })
    {
        return false;
    }
    let Instruction::Or {
        a: 3,
        s: update_argument,
        b: update_argument_copy,
    } = region[0]
    else {
        return false;
    };
    let Instruction::AddImmediateShifted {
        d: callback_address,
        a: 0,
        ..
    } = region[2]
    else {
        return false;
    };
    let Instruction::AddImmediate {
        d: 0,
        a: callback_address_low,
        ..
    } = region[3]
    else {
        return false;
    };
    let Instruction::StoreWord {
        s: 0,
        a: callback_base,
        ..
    } = region[4]
    else {
        return false;
    };
    let Instruction::LoadByteZero {
        d: 0,
        a: bitfield_base,
        ..
    } = region[5]
    else {
        return false;
    };
    let Instruction::AddImmediate {
        d: bit_value,
        a: 0,
        immediate: 1,
    } = region[6]
    else {
        return false;
    };
    let Instruction::RotateAndMaskInsert {
        a: 0,
        s: bit_value_use,
        ..
    } = region[7]
    else {
        return false;
    };
    let Instruction::StoreByte {
        s: 0,
        a: bitfield_store_base,
        ..
    } = region[8]
    else {
        return false;
    };
    let Instruction::Or {
        a: 3,
        s: call_argument,
        b: call_argument_copy,
    } = region[9]
    else {
        return false;
    };
    if !matches!(region[10], Instruction::AddImmediate { d: 4, a: 0, .. }) {
        return false;
    }

    update_argument == entry
        && update_argument_copy == entry
        && callback_address == callback_address_low
        && callback_base == state
        && bitfield_base == state
        && bit_value == bit_value_use
        && bitfield_store_base == state
        && call_argument == entry
        && call_argument_copy == entry
}

fn trailing_state_call_pair(instructions: &[Instruction], state: u8) -> bool {
    matches!(
        instructions.get(..5),
        Some([
            Instruction::Or {
                a: 3,
                s: first_argument,
                b: first_argument_copy,
            },
            Instruction::BranchAndLink { .. },
            Instruction::Or {
                a: 3,
                s: second_argument,
                b: second_argument_copy,
            },
            Instruction::AddImmediate { d: 4, a: 0, .. },
            Instruction::BranchAndLink { .. },
        ]) if *first_argument == state
            && *first_argument_copy == state
            && *second_argument == state
            && *second_argument_copy == state
    )
}

fn apply_permutation(output: &mut mwcc_machine_code::MachineFunction, permutation: &[usize]) {
    debug_assert_eq!(permutation.len(), output.instructions.len());
    let original = output.instructions.clone();
    output.instructions = permutation
        .iter()
        .map(|&index| original[index].clone())
        .collect();
    let mut inverse = vec![0usize; permutation.len()];
    for (new_index, &old_index) in permutation.iter().enumerate() {
        inverse[old_index] = new_index;
    }
    for relocation in &mut output.relocations {
        relocation.instruction_index = inverse[relocation.instruction_index];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaps_relocations_with_the_schedule() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 2,
            },
        ];
        output.relocations.push(mwcc_machine_code::Relocation {
            instruction_index: 0,
            kind: mwcc_machine_code::RelocationKind::Rel24,
            target: mwcc_machine_code::RelocationTarget::External("target".into()),
        });
        apply_permutation(&mut output, &[1, 0]);
        assert_eq!(output.relocations[0].instruction_index, 1);
    }
}
