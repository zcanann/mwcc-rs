//! Interleave a three-member float product with its following clamp.
//!
//! Build 163 keeps the first two factors in f2/f1, issues the clamp's zero
//! literal before their multiply, and delays the final multiply until after the
//! compare.  The product retains its virtual saved home; only the two short
//! factor lanes are fixed, so allocation can still color the surrounding loop.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

impl Generator {
    pub(crate) fn schedule_structured_effecter_product_clamp(&mut self) -> bool {
        let Some(plan) = plan(&self.output) else {
            return false;
        };
        apply_schedule(&mut self.output, plan);
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    start: usize,
    owner: u8,
    offsets: [i16; 3],
    product: u8,
    compared: u8,
}

fn plan(output: &MachineFunction) -> Option<Plan> {
    output.instructions.windows(8).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadFloatSingle { d: product, a: owner, offset: first },
            Instruction::LoadFloatSingle { d: temporary, a: second_owner, offset: second },
            Instruction::FloatMultiplySingle { d: first_result, a: first_factor, c: second_factor },
            Instruction::LoadFloatSingle { d: third_temporary, a: third_owner, offset: third },
            Instruction::FloatMultiplySingle { d: final_result, a: partial, c: third_factor },
            Instruction::LoadFloatSingle { d: zero, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: compared, b: compared_zero },
            Instruction::BranchConditionalForward { .. },
        ] = window
        else {
            return None;
        };
        (*owner != 0
            && owner == second_owner
            && owner == third_owner
            && product == first_result
            && product == first_factor
            && temporary == second_factor
            && temporary == third_temporary
            && product == final_result
            && product == partial
            && temporary == third_factor
            && *zero == 0
            && compared_zero == zero
            && *product > 31
            && *compared > 31
            && product != compared
            && loads_single_constant(output, start + 5, 0.0f32.to_bits())
            && !has_internal_entry(output, start + 1, start + 8))
            .then_some(Plan {
                start,
                owner: *owner,
                offsets: [*first, *second, *third],
                product: *product,
                compared: *compared,
            })
    })
}

fn apply_schedule(output: &mut MachineFunction, plan: Plan) {
    let start = plan.start;
    output.instructions[start] = Instruction::LoadFloatSingle {
        d: 2,
        a: plan.owner,
        offset: plan.offsets[0],
    };
    output.instructions[start + 1] = Instruction::LoadFloatSingle {
        d: 1,
        a: plan.owner,
        offset: plan.offsets[1],
    };
    output.instructions[start + 2] =
        Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 };
    output.instructions[start + 3] = Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 };
    output.instructions[start + 4] = Instruction::LoadFloatSingle {
        d: 2,
        a: plan.owner,
        offset: plan.offsets[2],
    };
    output.instructions[start + 5] =
        Instruction::FloatCompareOrdered { a: plan.compared, b: 0 };
    output.instructions[start + 6] =
        Instruction::FloatMultiplySingle { d: plan.product, a: 2, c: 1 };
    for relocation in &mut output.relocations {
        if relocation.instruction_index == start + 5 {
            relocation.instruction_index = start + 2;
        }
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

fn has_internal_entry(output: &MachineFunction, begin: usize, end: usize) -> bool {
    let branch_enters = output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                if (begin..end).contains(target)
        )
    });
    let table_enters = output.jump_tables.iter().any(|table| {
        table.entries.iter().any(|entry| {
            let destination = *entry as usize / 4;
            (begin..end).contains(&destination)
        })
    });
    branch_enters || table_enters
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    fn product_before_clamp() -> MachineFunction {
        let mut output = MachineFunction::default();
        let zero = output.intern_constant(u64::from(0.0f32.to_bits()), 4);
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 40, a: 30, offset: 180 },
            Instruction::LoadFloatSingle { d: 0, a: 30, offset: 240 },
            Instruction::FloatMultiplySingle { d: 40, a: 40, c: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 30, offset: 260 },
            Instruction::FloatMultiplySingle { d: 40, a: 40, c: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 41, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 12 },
        ];
        output.relocations.push(Relocation {
            instruction_index: 5,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(zero),
        });
        output
    }

    #[test]
    fn recognizes_and_interleaves_a_three_factor_product() {
        let mut output = product_before_clamp();
        let schedule = plan(&output).expect("the product/clamp region should match");
        apply_schedule(&mut output, schedule);
        assert_eq!(
            &output.instructions[..7],
            &[
                Instruction::LoadFloatSingle { d: 2, a: 30, offset: 180 },
                Instruction::LoadFloatSingle { d: 1, a: 30, offset: 240 },
                Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
                Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
                Instruction::LoadFloatSingle { d: 2, a: 30, offset: 260 },
                Instruction::FloatCompareOrdered { a: 41, b: 0 },
                Instruction::FloatMultiplySingle { d: 40, a: 2, c: 1 },
            ]
        );
        assert_eq!(output.relocations[0].instruction_index, 2);
    }

    #[test]
    fn rejects_a_region_with_an_internal_control_flow_entry() {
        let mut output = product_before_clamp();
        output.instructions.push(Instruction::Branch { target: 3 });
        assert_eq!(plan(&output), None);
    }
}
