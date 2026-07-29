//! Straight-line reuse of repeatedly materialized integer constants.
//!
//! Store lowering normally uses `r0` as a short-lived constant scratch. When a
//! longer store group also needs `r0` for a read/modify/write, MWCC instead keeps
//! the constant in the first otherwise-unused volatile GPR. This pass operates
//! on the final physical stream so the complete live range and every alternate
//! control-flow entry are explicit.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone)]
struct ConstantRange {
    load: usize,
    destination: u8,
    immediate: i16,
    uses: Vec<usize>,
}

#[derive(Debug)]
struct ConstantReuse {
    ranges: Vec<ConstantRange>,
    register: u8,
}

impl Generator {
    pub(crate) fn reuse_repeated_integer_constants(&mut self) {
        while let Some(plan) = repeated_integer_constant_reuse(&self.output) {
            let first = &plan.ranges[0];
            if let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[first.load]
            {
                *d = plan.register;
            }
            for range in &plan.ranges {
                for &index in &range.uses {
                    mwcc_vreg::for_each_register(
                        &mut self.output.instructions[index],
                        |role, class, register| {
                            if role == mwcc_vreg::RegisterRole::Use
                                && class == mwcc_vreg::Class::General
                                && *register == range.destination
                            {
                                *register = plan.register;
                            }
                        },
                    );
                }
            }
            for index in plan
                .ranges
                .iter()
                .skip(1)
                .map(|range| range.load)
                .rev()
            {
                crate::remove_instruction_retargeting_to_next(self, index);
            }
        }
    }
}

fn repeated_integer_constant_reuse(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<ConstantReuse> {
    let ranges = constant_ranges(&output.instructions);
    for first in 0..ranges.len() {
        let block_end = next_control_flow(&output.instructions, ranges[first].load);
        let matching: Vec<_> = ranges[first..]
            .iter()
            .take_while(|range| range.load < block_end)
            .filter(|range| {
                range.immediate == ranges[first].immediate && !range.uses.is_empty()
            })
            .cloned()
            .collect();
        // Two isolated materializations are often intentional scheduling.
        // Three or more establish the retained-constant store pattern.
        if matching.len() < 3 {
            continue;
        }
        let first_load = matching[0].load;
        let last_use = matching
            .iter()
            .flat_map(|range| range.uses.iter().copied())
            .max()?;
        if has_alternate_entry(&output.instructions, first_load + 1..last_use + 1) {
            continue;
        }
        if let Some(register) = (3u8..=10).find(|&candidate| {
            register_is_available_for_ranges(
                &output.instructions,
                &matching,
                candidate,
                first_load,
                last_use,
                block_end,
            )
        }) {
            return Some(ConstantReuse {
                ranges: matching,
                register,
            });
        }
    }
    None
}

fn constant_ranges(instructions: &[Instruction]) -> Vec<ConstantRange> {
    let mut ranges = Vec::new();
    for (load, instruction) in instructions.iter().enumerate() {
        let Instruction::AddImmediate {
            d: destination,
            a: 0,
            immediate,
        } = instruction
        else {
            continue;
        };
        let mut uses = Vec::new();
        let mut index = load + 1;
        while index < instructions.len() && !is_control_flow(&instructions[index]) {
            let operands = mwcc_vreg::register_operands(&instructions[index]);
            if operands.iter().any(|operand| {
                operand.role == mwcc_vreg::RegisterRole::Use
                    && operand.class == mwcc_vreg::Class::General
                    && operand.register == *destination
            }) {
                uses.push(index);
            }
            index += 1;
            if operands.iter().any(|operand| {
                operand.role == mwcc_vreg::RegisterRole::Define
                    && operand.class == mwcc_vreg::Class::General
                    && operand.register == *destination
            }) {
                break;
            }
        }
        ranges.push(ConstantRange {
            load,
            destination: *destination,
            immediate: *immediate,
            uses,
        });
    }
    ranges
}

fn register_is_available_for_ranges(
    instructions: &[Instruction],
    ranges: &[ConstantRange],
    candidate: u8,
    first_load: usize,
    last_use: usize,
    block_end: usize,
) -> bool {
    for (index, instruction) in instructions
        .iter()
        .enumerate()
        .take(last_use + 1)
        .skip(first_load)
    {
        for operand in mwcc_vreg::register_operands(instruction) {
            if operand.class != mwcc_vreg::Class::General || operand.register != candidate {
                continue;
            }
            let allowed_load = operand.role == mwcc_vreg::RegisterRole::Define
                && ranges
                    .iter()
                    .any(|range| range.load == index && range.destination == candidate);
            let allowed_use = operand.role == mwcc_vreg::RegisterRole::Use
                && ranges.iter().any(|range| {
                    range.destination == candidate && range.uses.contains(&index)
                });
            if !allowed_load && !allowed_use {
                return false;
            }
        }
    }

    // Do not destroy a value that remains live after the merged constant's last
    // use. A later definition proves the volatile register is dead; a call also
    // kills it implicitly. Branch boundaries remain conservative.
    for instruction in &instructions[last_use + 1..block_end] {
        let operands = mwcc_vreg::register_operands(instruction);
        if operands.iter().any(|operand| {
            operand.role == mwcc_vreg::RegisterRole::Use
                && operand.class == mwcc_vreg::Class::General
                && operand.register == candidate
        }) {
            return false;
        }
        if operands.iter().any(|operand| {
            operand.role == mwcc_vreg::RegisterRole::Define
                && operand.class == mwcc_vreg::Class::General
                && operand.register == candidate
        }) {
            return true;
        }
    }
    // Calls and returns have implicit ABI uses which the instruction operand
    // description intentionally omits. Without an explicit intervening
    // definition, the candidate could still be an argument or result.
    false
}

fn next_control_flow(instructions: &[Instruction], start: usize) -> usize {
    instructions[start + 1..]
        .iter()
        .position(is_control_flow)
        .map_or(instructions.len(), |offset| start + 1 + offset)
}

fn is_control_flow(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::BranchConditionalForward { .. }
            | Instruction::Branch { .. }
            | Instruction::BranchConditionalToLinkRegister { .. }
            | Instruction::BranchToLinkRegister
            | Instruction::BranchToLinkRegisterAndLink
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchToCountRegister
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::ReturnFromInterrupt
            | Instruction::SystemCall
    )
}

fn has_alternate_entry(instructions: &[Instruction], region: std::ops::Range<usize>) -> bool {
    instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                if region.contains(target)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn li(register: u8) -> Instruction {
        Instruction::AddImmediate {
            d: register,
            a: 0,
            immediate: 0,
        }
    }

    fn store(register: u8, offset: i16) -> Instruction {
        Instruction::StoreWord {
            s: register,
            a: 31,
            offset,
        }
    }

    #[test]
    fn retains_three_straight_line_zero_ranges_in_the_first_free_register() {
        let output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                li(0),
                store(0, 0),
                Instruction::LoadByteZero {
                    d: 0,
                    a: 31,
                    offset: 4,
                },
                li(3),
                Instruction::RotateAndMaskInsert {
                    a: 0,
                    s: 3,
                    shift: 7,
                    begin: 24,
                    end: 24,
                },
                li(0),
                store(0, 8),
                Instruction::LoadWord {
                    d: 3,
                    a: 31,
                    offset: 0,
                },
            ],
            ..Default::default()
        };

        let plan = repeated_integer_constant_reuse(&output).expect("repeated zero plan");
        assert_eq!(plan.register, 3);
        assert_eq!(
            plan.ranges.iter().map(|range| range.load).collect::<Vec<_>>(),
            [0, 3, 5]
        );
    }

    #[test]
    fn skips_a_live_candidate_and_uses_the_next_register() {
        let output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                li(0),
                store(0, 0),
                li(0),
                store(0, 4),
                li(0),
                store(0, 8),
                Instruction::StoreWord {
                    s: 3,
                    a: 31,
                    offset: 12,
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 31,
                    offset: 16,
                },
                Instruction::BranchToLinkRegister,
            ],
            ..Default::default()
        };

        let plan = repeated_integer_constant_reuse(&output).expect("another free register exists");
        assert_eq!(plan.register, 4);
    }

    #[test]
    fn rejects_an_alternate_entry_after_the_retained_load() {
        let output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                li(0),
                store(0, 0),
                li(0),
                store(0, 4),
                li(0),
                store(0, 8),
                Instruction::BranchToLinkRegister,
                Instruction::Branch { target: 2 },
            ],
            ..Default::default()
        };

        assert!(repeated_integer_constant_reuse(&output).is_none());
    }
}
