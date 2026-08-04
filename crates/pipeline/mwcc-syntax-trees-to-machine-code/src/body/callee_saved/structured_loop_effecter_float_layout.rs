//! Saved-FPR layout for a dense effecter-mixing loop.
//!
//! Once instruction selection exposes the complete loop, its constants,
//! complements, switch selection, product, and conversion bias have stable
//! semantic roles. MWCC assigns those roles a non-source-order saved-FPR layout;
//! expressing that layout as allocator preferences avoids pinning instructions
//! or adding a special allocator mode.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    one: u8,
    zero: u8,
    complements: [u8; 3],
    bias: u8,
    selected: u8,
    accumulator: u8,
    product: u8,
    third_input: u8,
}

impl Generator {
    pub(super) fn prefer_structured_effecter_loop_float_layout(
        &mut self,
        insertion: usize,
        bias_load: usize,
    ) {
        let Some(layout) = layout(&self.output, insertion, bias_load) else {
            return;
        };
        self.prefer_virtual_float(layout.one, 24);
        self.prefer_virtual_float(layout.zero, 25);
        for (register, preferred) in layout.complements.into_iter().zip([23, 22, 21]) {
            self.prefer_virtual_float(register, preferred);
        }
        self.prefer_virtual_float(layout.bias, 26);
        self.prefer_virtual_float(layout.selected, 27);
        self.prefer_virtual_float(layout.accumulator, 20);
        self.prefer_virtual_float(layout.product, 30);
        self.prefer_virtual_float(layout.third_input, 28);
    }
}

fn layout(output: &MachineFunction, insertion: usize, bias_load: usize) -> Option<Layout> {
    let search_start = insertion.saturating_sub(20);
    let one_load = (search_start..insertion)
        .rev()
        .find(|index| loads_single_constant(output, *index, 1.0f32.to_bits()))?;
    let zero_load = (search_start..insertion)
        .rev()
        .find(|index| loads_single_constant(output, *index, 0.0f32.to_bits()))?;
    let one = loaded_single_destination(output, one_load)?;
    let zero = loaded_single_destination(output, zero_load)?;
    let complement_pairs = (one_load.min(zero_load) + 1..insertion)
        .filter_map(|index| match output.instructions[index] {
            Instruction::FloatSubtractSingle { d, a, b } if a == one => Some((d, b)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(first, _), (second, _), (third, third_input)] = complement_pairs.as_slice() else {
        return None;
    };

    let backedge = output.instructions[bias_load + 1..]
        .iter()
        .enumerate()
        .find_map(|(relative, instruction)| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. }
                if *target <= bias_load =>
            {
                Some(bias_load + 1 + relative)
            }
            _ => None,
        })?;
    let (call, selected) = (insertion..backedge).find_map(|index| {
        let Instruction::FloatMove { d: 1, b } = output.instructions[index] else {
            return None;
        };
        matches!(
            output.instructions.get(index + 1),
            Some(Instruction::BranchAndLink { .. })
        )
        .then_some((index + 1, b))
    })?;
    let accumulator = (call + 1..(call + 4).min(backedge)).find_map(|index| {
        match output.instructions[index] {
            Instruction::FloatMultiplySingle { d, a, c } if d == a && c == 1 => Some(d),
            Instruction::FloatMultiplySingle { d, a, c } if d == c && a == 1 => Some(d),
            _ => None,
        }
    })?;
    let product = (insertion..call).find_map(|index| match output.instructions[index] {
        Instruction::FloatMove { d, b } if d == accumulator => Some(b),
        _ => None,
    })?;
    let bias = match output.instructions[bias_load] {
        Instruction::LoadFloatDouble { d, .. } => d,
        _ => return None,
    };

    Some(Layout {
        one,
        zero,
        complements: [*first, *second, *third],
        bias,
        selected,
        accumulator,
        product,
        third_input: *third_input,
    })
}

fn loaded_single_destination(output: &MachineFunction, instruction: usize) -> Option<u8> {
    match output.instructions[instruction] {
        Instruction::LoadFloatSingle { d, .. } => Some(d),
        _ => None,
    }
}

fn loads_single_constant(output: &MachineFunction, instruction: usize, bits: u32) -> bool {
    output.relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction
            && relocation.kind == RelocationKind::EmbSda21
            && matches!(
                relocation.target,
                RelocationTarget::Constant(constant)
                    if output.constants.get(constant).is_some_and(|constant| {
                        constant.byte_width == 4 && constant.bits == u64::from(bits)
                    })
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    #[test]
    fn recovers_each_saved_float_role_from_the_complete_loop() {
        let mut output = MachineFunction::default();
        let one = output.intern_constant(u64::from(1.0f32.to_bits()), 4);
        let zero = output.intern_constant(u64::from(0.0f32.to_bits()), 4);
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 40, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 41, a: 0, offset: 0 },
            Instruction::FloatSubtractSingle { d: 42, a: 40, b: 50 },
            Instruction::FloatSubtractSingle { d: 43, a: 40, b: 51 },
            Instruction::FloatSubtractSingle { d: 44, a: 40, b: 52 },
            Instruction::FloatMove { d: 45, b: 46 },
            Instruction::FloatMove { d: 1, b: 47 },
            Instruction::BranchAndLink { target: "sample".into() },
            Instruction::FloatMultiplySingle { d: 45, a: 45, c: 1 },
            Instruction::LoadFloatDouble { d: 48, a: 0, offset: 0 },
            Instruction::StoreWord { s: 28, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 48 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 5 },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(one),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
        ];

        assert_eq!(
            layout(&output, 5, 9),
            Some(Layout {
                one: 40,
                zero: 41,
                complements: [42, 43, 44],
                bias: 48,
                selected: 47,
                accumulator: 45,
                product: 46,
                third_input: 52,
            })
        );
    }
}
