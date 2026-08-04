//! Retained integer-conversion bias in a call-making loop.
//!
//! The `0x4330000000000000` bias paired with a retained `0x4330` image word is
//! loop invariant. MWCC loads it once into a saved FPR and reuses it after each
//! call instead of issuing an SDA load on every iteration.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

const UNSIGNED_BIAS: u64 = 0x4330_0000_0000_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    insertion: usize,
    load: usize,
    subtract: usize,
}

impl Generator {
    pub(crate) fn hoist_structured_loop_conversion_bias(&mut self, retained: u8) -> bool {
        let Some(plan) = plan(&self.output) else {
            return false;
        };
        self.prefer_structured_effecter_loop_float_layout(plan.insertion, plan.load);
        let old = match self.output.instructions[plan.load] {
            Instruction::LoadFloatDouble { d, .. } => d,
            _ => unreachable!("conversion bias load changed after recognition"),
        };
        let Instruction::LoadFloatDouble { d, .. } = &mut self.output.instructions[plan.load]
        else {
            unreachable!("conversion bias load changed after recognition")
        };
        *d = retained;
        match &mut self.output.instructions[plan.subtract] {
            Instruction::FloatSubtractSingle { a, b, .. }
            | Instruction::FloatSubtractDouble { a, b, .. } => {
                if *a == old {
                    *a = retained;
                }
                if *b == old {
                    *b = retained;
                }
            }
            _ => unreachable!("conversion bias subtraction changed after recognition"),
        }
        crate::move_instruction_before_retargeting_source_to_next(
            self,
            plan.load,
            plan.insertion,
        );
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }
}

fn plan(output: &MachineFunction) -> Option<Plan> {
    for load in 0..output.instructions.len() {
        let bias = match output.instructions[load] {
            Instruction::LoadFloatDouble {
                d,
                a: 0,
                offset: 0,
            } => d,
            _ => continue,
        };
        if !loads_bias(output, load) {
            continue;
        }
        let Some((backedge, insertion)) = output.instructions[load + 1..]
            .iter()
            .enumerate()
            .find_map(|(relative, instruction)| match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target <= load =>
                {
                    Some((load + 1 + relative, *target))
                }
                _ => None,
            })
        else {
            continue;
        };
        if !output.instructions[insertion..backedge]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        {
            continue;
        }
        let Some(subtract) = (load + 1..(load + 8).min(backedge)).find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::FloatSubtractSingle { a, b, .. }
                    | Instruction::FloatSubtractDouble { a, b, .. }
                    if a == bias || b == bias
            )
        }) else {
            continue;
        };
        return Some(Plan {
            insertion,
            load,
            subtract,
        });
    }
    None
}

fn loads_bias(output: &MachineFunction, instruction: usize) -> bool {
    output.relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction
            && relocation.kind == RelocationKind::EmbSda21
            && matches!(
                relocation.target,
                RelocationTarget::Constant(constant)
                    if output.constants.get(constant).is_some_and(|constant| {
                        constant.byte_width == 8 && constant.bits == UNSIGNED_BIAS
                    })
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    #[test]
    fn recognizes_a_bias_load_consumed_after_a_loop_call() {
        let mut output = MachineFunction::default();
        let bias = output.intern_constant(UNSIGNED_BIAS, 8);
        output.instructions = vec![
            Instruction::load_immediate(31, 0),
            Instruction::Add { d: 25, a: 30, b: 31 },
            Instruction::BranchAndLink { target: "sample".into() },
            Instruction::LoadFloatDouble { d: 1, a: 0, offset: 0 },
            Instruction::StoreWord { s: 28, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 },
            Instruction::AddImmediate { d: 31, a: 31, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 6 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 1 },
        ];
        output.relocations.push(Relocation {
            instruction_index: 3,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(bias),
        });
        assert_eq!(
            plan(&output),
            Some(Plan { insertion: 1, load: 3, subtract: 6 })
        );
    }
}
