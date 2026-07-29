//! Callback setup scheduling for dense path-colored conditional bodies.
//!
//! In the first guarded arm, MWCC starts materializing a callback while the
//! preceding call is in flight. It lets the callback address flow through r3
//! into r4, then installs the retained object receiver in r3. The returned
//! process object's flag merge likewise starts its independent global load
//! before loading the destination byte.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_callback_setup(&mut self) {
        let Some((permutation, start, moved_flag_load)) =
            rewrite_callback_setup(&mut self.output)
        else {
            return;
        };
        self.labels.moved_before(start + 1, start);
        self.labels.moved_before(start + 2, start + 1);
        if moved_flag_load {
            self.labels.moved_before(start + 7, start + 6);
        }
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn rewrite_callback_setup(
    output: &mut mwcc_machine_code::MachineFunction,
) -> Option<(Vec<usize>, usize, bool)> {
    let start = callback_setup_start(output)?;
    {
        let Instruction::AddImmediateShifted { a, immediate, .. } =
            output.instructions[start + 1]
        else {
            unreachable!("the callback high half was recognized above");
        };
        output.instructions[start + 1] = Instruction::AddImmediateShifted {
            d: 3,
            a,
            immediate,
        };
        let Instruction::AddImmediate { immediate, .. } =
            output.instructions[start + 2]
        else {
            unreachable!("the callback low half was recognized above");
        };
        output.instructions[start + 2] = Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate,
        };
    }

    let moved_flag_load = output.instructions.len() >= start + 10
        && matches!(
            &output.instructions[start + 5..start + 10],
            [
                Instruction::Or {
                    a: result,
                    s: 3,
                    b: 3,
                }
                | Instruction::AddImmediate {
                    d: result,
                    a: 3,
                    immediate: 0,
                },
                Instruction::LoadByteZero {
                    d: byte,
                    a: byte_base,
                    offset: 13,
                },
                Instruction::LoadWord { d: flags, .. },
                Instruction::RotateAndMaskInsert {
                    a: merged,
                    s: inserted,
                    shift: 4,
                    begin: 26,
                    end: 27,
                },
                Instruction::StoreByte {
                    s: stored,
                    a: store_base,
                    offset: 13,
                },
            ] if result == byte_base
                && result == store_base
                && byte == merged
                && byte == stored
                && flags == inserted
        );
    let order: &[usize] = if moved_flag_load {
        &[1, 2, 0, 3, 4, 5, 7, 6, 8, 9]
    } else {
        &[1, 2, 0, 3, 4]
    };
    let original = output.instructions[start..start + order.len()].to_vec();
    for (new_offset, &old_offset) in order.iter().enumerate() {
        output.instructions[start + new_offset] = original[old_offset].clone();
    }

    let mut permutation: Vec<_> = (0..output.instructions.len()).collect();
    for (new_offset, &old_offset) in order.iter().enumerate() {
        permutation[start + old_offset] = start + new_offset;
    }
    Some((permutation, start, moved_flag_load))
}

fn callback_setup_start(output: &mwcc_machine_code::MachineFunction) -> Option<usize> {
    let start = output.instructions.windows(5).position(|window| {
        let [
            receiver_copy,
            Instruction::AddImmediateShifted {
                d: address,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: low,
                a: address_base,
                ..
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::BranchAndLink { .. },
        ] = window
        else {
            return false;
        };
        let receiver_copy = match receiver_copy {
            Instruction::Or {
                a: 3,
                s: receiver,
                b,
            } => receiver == b,
            Instruction::AddImmediate {
                d: 3,
                immediate: 0,
                ..
            } => true,
            _ => false,
        };
        receiver_copy && address == low && address == address_base
    })?;

    let high = output.relocations.iter().find(|relocation| {
        relocation.instruction_index == start + 1
            && relocation.kind == RelocationKind::Addr16Ha
    })?;
    let low = output.relocations.iter().find(|relocation| {
        relocation.instruction_index == start + 2
            && relocation.kind == RelocationKind::Addr16Lo
    })?;
    match (&high.target, &low.target) {
        (
            mwcc_machine_code::RelocationTarget::External(high),
            mwcc_machine_code::RelocationTarget::External(low),
        ) if high == low => Some(start),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn materializes_callback_before_its_retained_receiver() {
        let receiver = mwcc_vreg::VIRTUAL_BASE;
        let address = receiver + 1;
        let result = receiver + 2;
        let byte = receiver + 3;
        let flags = receiver + 4;
        let mut output = mwcc_machine_code::MachineFunction::new("callback");
        output.instructions = vec![
            Instruction::move_register(3, receiver),
            Instruction::load_immediate_shifted(address, 0),
            Instruction::AddImmediate {
                d: address,
                a: address,
                immediate: 0,
            },
            Instruction::load_immediate(5, 0),
            Instruction::BranchAndLink {
                target: "install".into(),
            },
            Instruction::move_register(result, 3),
            Instruction::LoadByteZero {
                d: byte,
                a: result,
                offset: 13,
            },
            Instruction::LoadWord {
                d: flags,
                a: 0,
                offset: 0,
            },
            Instruction::RotateAndMaskInsert {
                a: byte,
                s: flags,
                shift: 4,
                begin: 26,
                end: 27,
            },
            Instruction::StoreByte {
                s: byte,
                a: result,
                offset: 13,
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".into()),
            },
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("callback".into()),
            },
            Relocation {
                instruction_index: 7,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External("flags".into()),
            },
        ];

        let (permutation, start, moved_flag_load) =
            rewrite_callback_setup(&mut output).unwrap();

        assert_eq!(start, 0);
        assert!(moved_flag_load);
        assert_eq!(permutation, [2, 0, 1, 3, 4, 5, 7, 6, 8, 9]);
        assert!(matches!(
            output.instructions[0],
            Instruction::AddImmediateShifted { d: 3, .. }
        ));
        assert!(matches!(
            output.instructions[1],
            Instruction::AddImmediate { d: 4, a: 3, .. }
        ));
        assert!(matches!(
            output.instructions[2],
            Instruction::Or { a: 3, s, b } if s == receiver && b == receiver
        ));
        assert!(matches!(
            output.instructions[6],
            Instruction::LoadWord { d, .. } if d == flags
        ));
    }
}
