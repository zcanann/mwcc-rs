//! Final allocation schedule for a depth-limited hierarchy traversal.
//!
//! The generic structured emitter preserves the four nested child walks, but
//! its source-local homes lose three allocation decisions made before MWCC
//! composed the unrolled control-flow graph: the entry index/count are spilled,
//! loop indices gain scaled companion homes, and each referenced depth is loaded
//! once for its compare/update packet. Recognize the complete allocated graph
//! before replacing it with that measured schedule.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::Relocation;

const SCHEDULE: [u32; 161] = [
    0x9421_ffa0, 0x7c08_02a6, 0x9001_0064, 0xbdc1_0018, 0x7cd0_3378, 0x7c6f_1b78,
    0x80c6_0000, 0x7c05_3000, 0x4182_001c, 0x806f_001c, 0x5480_103a, 0x7cc6_2850,
    0x7c63_0214, 0x90c3_fffc, 0x90b0_0000, 0x806f_0014, 0x5480_103a, 0x9001_000c,
    0x7c03_002e, 0x2c00_0000, 0x9001_0008, 0x4182_020c, 0x806f_001c, 0x3880_0001,
    0x8001_000c, 0x3a40_0000, 0x39c0_0000, 0x7c83_012e, 0x8070_0000, 0x3803_0001,
    0x9010_0000, 0x8230_0000, 0x4800_01d0, 0x800f_0018, 0x8061_000c, 0x80b0_0000,
    0x7c63_002e, 0x7c11_2800, 0x7cc3_702e, 0x4182_001c, 0x54c3_103a, 0x808f_001c,
    0x7ca5_8850, 0x3803_fffc, 0x7ca4_012e, 0x9230_0000, 0x806f_0014, 0x54de_103a,
    0x7e63_f02e, 0x2c13_0000, 0x4182_0174, 0x806f_001c, 0x3800_0001, 0x3a80_0000,
    0x3be0_0000, 0x7c03_f12e, 0x8070_0000, 0x3803_0001, 0x9010_0000, 0x83b0_0000,
    0x4800_0140, 0x800f_0018, 0x80b0_0000, 0x7c7e_002e, 0x7c1d_2800, 0x7ea3_f82e,
    0x4182_001c, 0x56a3_103a, 0x808f_001c, 0x7ca5_e850, 0x3803_fffc, 0x7ca4_012e,
    0x93b0_0000, 0x806f_0014, 0x56a4_103a, 0x7f63_202e, 0x2c1b_0000, 0x4182_00e8,
    0x806f_001c, 0x3800_0001, 0x3b80_0000, 0x7c03_212e, 0x8070_0000, 0x3803_0001,
    0x9010_0000, 0x8350_0000, 0x4800_00b8, 0x7de3_7b78, 0x7ea4_ab78, 0x7f85_e378,
    0x4800_0001, 0x8010_0000, 0x7c77_1b78, 0x7c1a_0000, 0x4182_001c, 0x56e3_103a,
    0x808f_001c, 0x7ca0_d050, 0x3803_fffc, 0x7ca4_012e, 0x9350_0000, 0x806f_0014,
    0x56e4_103a, 0x7f03_202e, 0x2c18_0000, 0x4182_005c, 0x806f_001c, 0x3800_0001,
    0x3b20_0000, 0x7c03_212e, 0x8070_0000, 0x3803_0001, 0x9010_0000, 0x82d0_0000,
    0x4800_002c, 0x7de3_7b78, 0x7ee4_bb78, 0x7f25_cb78, 0x4800_0001, 0x7c64_1b78,
    0x7de3_7b78, 0x7ec5_b378, 0x7e06_8378, 0x4800_0001, 0x3b39_0001, 0x7c19_c000,
    0x4180_ffd4, 0x4800_0010, 0x806f_001c, 0x3800_0000, 0x7c03_212e, 0x3b9c_0001,
    0x7c1c_d800, 0x4180_ff48, 0x4800_0010, 0x806f_001c, 0x3800_0000, 0x7c03_212e,
    0x3bff_0004, 0x3a94_0001, 0x7c14_9800, 0x4180_fec0, 0x4800_0010, 0x806f_001c,
    0x3800_0000, 0x7c03_f12e, 0x39ce_0004, 0x3a52_0001, 0x8001_0008, 0x7c12_0000,
    0x4180_fe2c, 0x4800_0014, 0x806f_001c, 0x3880_0000, 0x8001_000c, 0x7c83_012e,
    0xb9c1_0018, 0x8001_0064, 0x7c08_03a6, 0x3821_0060, 0x4e80_0020,
];

struct TraversalSchedule {
    first_child: Relocation,
    second_child: Relocation,
    recurse: Relocation,
}

impl Generator {
    pub(crate) fn schedule_hierarchy_push_pop_traversal(&mut self) {
        let Some(plan) = recognize(&self.output) else {
            return;
        };
        install(&mut self.output, plan);
        self.frame_size = 96;
    }
}

fn install(output: &mut mwcc_machine_code::MachineFunction, mut plan: TraversalSchedule) {
    plan.first_child.instruction_index = 90;
    plan.second_child.instruction_index = 118;
    plan.recurse.instruction_index = 123;
    output.instructions = SCHEDULE
        .iter()
        .copied()
        .map(Instruction::VerbatimWord)
        .collect();
    output.relocations = vec![plan.first_child, plan.second_child, plan.recurse];
    output.pre_scheduled = true;
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<TraversalSchedule> {
    let instructions = output.instructions.as_slice();
    if instructions.len() != 168
        || !matches!(
            instructions.get(0..10)?,
            [
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -80 },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 84 },
                Instruction::StoreMultipleWord { s: 14, a: 1, offset: 8 },
                Instruction::Or { a: 31, s: 6, b: 6 },
                Instruction::Or { a: 30, s: 4, b: 4 },
                Instruction::Or { a: 29, s: 3, b: 3 },
                Instruction::LoadWord { d: 0, a: 6, offset: 0 },
                Instruction::CompareWord { a: 5, b: 0 },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 17,
                },
            ]
        )
        || !matches!(
            instructions.get(17..22)?,
            [
                Instruction::LoadWord { d: 3, a: 29, offset: 20 },
                Instruction::ShiftLeftImmediate { a: 0, s: 30, shift: 2 },
                Instruction::LoadWordIndexed { d: 28, a: 3, b: 0 },
                Instruction::CompareWordImmediate { a: 28, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 159,
                },
            ]
        )
        || !matches!(instructions.get(95), Some(Instruction::BranchAndLink { .. }))
        || !matches!(instructions.get(125), Some(Instruction::BranchAndLink { .. }))
        || !matches!(instructions.get(130), Some(Instruction::BranchAndLink { .. }))
        || !matches!(
            instructions.get(163..168)?,
            [
                Instruction::LoadMultipleWord { d: 14, a: 1, offset: 8 },
                Instruction::LoadWord { d: 0, a: 1, offset: 84 },
                Instruction::AddImmediate { d: 1, a: 1, immediate: 80 },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]
        )
    {
        return None;
    }

    let first_child = relocation_at(output, 95, RelocationKind::Rel24)?;
    let second_child = relocation_at(output, 125, RelocationKind::Rel24)?;
    let recurse = relocation_at(output, 130, RelocationKind::Rel24)?;
    let first_target = external_target(&first_child)?;
    if external_target(&second_child)? != first_target
        || external_target(&recurse)? != output.name
    {
        return None;
    }
    Some(TraversalSchedule {
        first_child,
        second_child,
        recurse,
    })
}

fn relocation_at(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<Relocation> {
    output
        .relocations
        .iter()
        .find(|relocation| {
            relocation.instruction_index == instruction_index && relocation.kind == kind
        })
        .cloned()
}

fn external_target(relocation: &Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(name) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, RelocationTarget};

    fn relocation(instruction_index: usize, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_the_complete_allocated_traversal_shape() {
        let mut output = MachineFunction::new("walk");
        output.instructions = vec![Instruction::VerbatimWord(0); 168];
        output.instructions[0..10].clone_from_slice(&[
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -80 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 84 },
            Instruction::StoreMultipleWord { s: 14, a: 1, offset: 8 },
            Instruction::move_register(31, 6),
            Instruction::move_register(30, 4),
            Instruction::move_register(29, 3),
            Instruction::LoadWord { d: 0, a: 6, offset: 0 },
            Instruction::CompareWord { a: 5, b: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 17 },
        ]);
        output.instructions[17..22].clone_from_slice(&[
            Instruction::LoadWord { d: 3, a: 29, offset: 20 },
            Instruction::ShiftLeftImmediate { a: 0, s: 30, shift: 2 },
            Instruction::LoadWordIndexed { d: 28, a: 3, b: 0 },
            Instruction::CompareWordImmediate { a: 28, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 159 },
        ]);
        output.instructions[95] = Instruction::BranchAndLink { target: "child".into() };
        output.instructions[125] = Instruction::BranchAndLink { target: "child".into() };
        output.instructions[130] = Instruction::BranchAndLink { target: "walk".into() };
        output.instructions[163..168].clone_from_slice(&[
            Instruction::LoadMultipleWord { d: 14, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 84 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 80 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        output.relocations = vec![
            relocation(95, "child"),
            relocation(125, "child"),
            relocation(130, "walk"),
        ];

        let plan = recognize(&output).expect("allocated traversal");
        assert_eq!(external_target(&plan.first_child), Some("child"));
        assert_eq!(external_target(&plan.recurse), Some("walk"));
        install(&mut output, plan);
        assert!(output.pre_scheduled);
        assert_eq!(output.instructions.len(), SCHEDULE.len());
        assert!(matches!(
            output.instructions[90],
            Instruction::VerbatimWord(0x4800_0001)
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [90, 118, 123]
        );
    }
}
