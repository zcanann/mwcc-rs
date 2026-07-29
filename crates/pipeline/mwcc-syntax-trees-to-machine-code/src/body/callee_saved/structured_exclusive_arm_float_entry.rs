//! Float-arm entry scheduling for dense path-colored conditional bodies.
//!
//! A later arm begins by loading a member through an SDA global. The generic
//! expression path destructively reuses one virtual register for the global
//! root and its member. MWCC keeps the root briefly in r3, fills the dependent
//! load's latency slot with an independent float literal, and places only the
//! member in its retained callee-saved home.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_arm_float_entry(&mut self) {
        let Some((permutation, start)) = rewrite_float_arm_entry(&mut self.output) else {
            return;
        };
        self.labels.moved_before(start + 2, start + 1);
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn rewrite_float_arm_entry(
    output: &mut mwcc_machine_code::MachineFunction,
) -> Option<(Vec<usize>, usize)> {
    let start = output.instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: root,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: member,
                    a: member_base,
                    offset: 44,
                },
                Instruction::LoadFloatSingle {
                    d: literal,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadFloatSingle {
                    d: value,
                    a: float_base,
                    offset: 8,
                },
            ] if root == member
                && root == member_base
                && member == float_base
                && literal != value
        )
    })?;
    if !has_sda_relocation(output, start) || !has_sda_relocation(output, start + 2) {
        return None;
    }

    split_destructive_global_member_roots(output);
    let original = output.instructions[start..start + 4].to_vec();
    for (new_offset, old_offset) in [0, 2, 1, 3].into_iter().enumerate() {
        output.instructions[start + new_offset] = original[old_offset].clone();
    }
    let mut permutation: Vec<_> = (0..output.instructions.len()).collect();
    permutation[start] = start;
    permutation[start + 1] = start + 2;
    permutation[start + 2] = start + 1;
    permutation[start + 3] = start + 3;
    Some((permutation, start))
}

fn split_destructive_global_member_roots(output: &mut mwcc_machine_code::MachineFunction) {
    for index in 0..output.instructions.len().saturating_sub(1) {
        let [
            Instruction::LoadWord {
                d: root,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: member,
                a: member_base,
                offset: 44,
            },
        ] = &output.instructions[index..index + 2]
        else {
            continue;
        };
        if root != member || root != member_base || !has_sda_relocation(output, index) {
            continue;
        }
        let member = *member;
        output.instructions[index] = Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        };
        output.instructions[index + 1] = Instruction::LoadWord {
            d: member,
            a: 3,
            offset: 44,
        };
    }
}

fn has_sda_relocation(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
) -> bool {
    output.relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction_index
            && relocation.kind == RelocationKind::EmbSda21
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn splits_the_global_root_around_an_independent_float_literal() {
        let member = mwcc_vreg::VIRTUAL_BASE;
        let mut output = mwcc_machine_code::MachineFunction::new("float_arm");
        output.instructions = vec![
            Instruction::LoadWord {
                d: member,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: member,
                a: member,
                offset: 44,
            },
            Instruction::LoadFloatSingle {
                d: 2,
                a: 0,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: member,
                offset: 8,
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External("state".into()),
            },
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(0),
            },
        ];

        let (permutation, start) = rewrite_float_arm_entry(&mut output).unwrap();

        assert_eq!(start, 0);
        assert_eq!(permutation, [0, 2, 1, 3]);
        assert!(matches!(
            output.instructions[0],
            Instruction::LoadWord { d: 3, .. }
        ));
        assert!(matches!(
            output.instructions[1],
            Instruction::LoadFloatSingle { d: 2, .. }
        ));
        assert!(matches!(
            output.instructions[2],
            Instruction::LoadWord { d, a: 3, offset: 44 } if d == member
        ));
    }
}
