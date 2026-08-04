//! Retained `0x4330` high word for integer-to-double conversion in a loop.
//!
//! The conversion image is rebuilt every iteration by generic lowering. MWCC
//! materializes its invariant high word once after induction initialization and
//! keeps it in the third saved-GPR lane owned by the loop home layout.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    insertion: usize,
    materialization: usize,
    store: usize,
}

impl Generator {
    pub(crate) fn hoist_structured_loop_integer_conversion_high(
        &mut self,
        retained: u8,
    ) -> bool {
        let Some(plan) = plan(&self.output) else {
            return false;
        };
        let old = match self.output.instructions[plan.materialization] {
            Instruction::AddImmediateShifted { d, .. } => d,
            _ => unreachable!("conversion high changed after recognition"),
        };
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[plan.materialization]
        else {
            unreachable!("conversion high changed after recognition")
        };
        *d = retained;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[plan.store] else {
            unreachable!("conversion image store changed after recognition")
        };
        debug_assert_eq!(*s, old);
        *s = retained;
        crate::move_instruction_before_retargeting_source_to_next(
            self,
            plan.materialization,
            plan.insertion,
        );
        true
    }
}

fn plan(output: &mwcc_machine_code::MachineFunction) -> Option<Plan> {
    for materialization in 0..output.instructions.len() {
        let old = match output.instructions[materialization] {
            Instruction::AddImmediateShifted {
                d,
                a: 0,
                immediate: 0x4330,
            } => d,
            _ => continue,
        };
        let (backedge, insertion) = output.instructions[materialization + 1..]
            .iter()
            .enumerate()
            .find_map(|(relative, instruction)| match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target <= materialization =>
                {
                    Some((materialization + 1 + relative, *target))
                }
                _ => None,
            })?;
        if !output.instructions[insertion..backedge]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        {
            continue;
        }
        let store = (materialization + 1..backedge).find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::StoreWord { s, a: 1, .. } if s == old
            )
        })?;
        let Instruction::StoreWord { offset, .. } = output.instructions[store] else {
            unreachable!("conversion image store was matched")
        };
        let load = (store + 1..(store + 4).min(backedge)).find(|index| {
            matches!(
                output.instructions[*index],
                Instruction::LoadFloatDouble { a: 1, offset: candidate, .. }
                    if candidate == offset
            )
        })?;
        if !output.instructions[load + 1..(load + 4).min(backedge)]
            .iter()
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::FloatSubtractSingle { .. }
                        | Instruction::FloatSubtractDouble { .. }
                )
            })
        {
            continue;
        }
        return Some(Plan {
            insertion,
            materialization,
            store,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_high_word_feeding_a_counted_loop_conversion_image() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = vec![
            Instruction::load_immediate(31, 0),
            Instruction::Add { d: 25, a: 30, b: 31 },
            Instruction::BranchAndLink { target: "sample".into() },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0x4330 },
            Instruction::StoreWord { s: 3, a: 1, offset: 12 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 0, a: 1, offset: 8 },
            Instruction::FloatSubtractDouble { d: 0, a: 0, b: 1 },
            Instruction::AddImmediate { d: 31, a: 31, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: 31, immediate: 6 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 1 },
        ];
        assert_eq!(
            plan(&output),
            Some(Plan { insertion: 1, materialization: 3, store: 5 })
        );
    }
}
