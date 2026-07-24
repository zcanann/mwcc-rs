//! Straight-line reuse of absolute-addressed floating pool literals.
//!
//! With `-sdata2 0`, a pool load is a `lis @ha; lfs/lfd @l(base)` pair. MWCC
//! keeps the loaded FPR live across later arithmetic in the same basic block
//! instead of repeating both instructions. This pass runs on the final physical
//! stream, where register clobbers and control-flow entry points are explicit.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_absolute_pooled_float_literals(&mut self) {
        if self.behavior.read_only_global_addressing != GlobalAddressing::Absolute {
            return;
        }
        while let Some((high, low)) = redundant_absolute_pool_load(&self.output) {
            self.remove_redundant_pool_load_instruction(low);
            self.remove_redundant_pool_load_instruction(high);
        }
    }

    fn remove_redundant_pool_load_instruction(&mut self, index: usize) {
        let old_len = self.output.instructions.len();
        self.output.instructions.remove(index);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < index {
                    old
                } else if old == index {
                    index.saturating_sub(1)
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PoolFloatWidth {
    Single,
    Double,
}

#[derive(Clone, Copy)]
struct AbsolutePoolLoad {
    high: usize,
    low: usize,
    destination: u8,
    width: PoolFloatWidth,
}

fn redundant_absolute_pool_load(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<(usize, usize)> {
    let loads: Vec<_> = (0..output.instructions.len().saturating_sub(1))
        .filter_map(|high| absolute_pool_load(output, high))
        .collect();
    for (position, first) in loads.iter().enumerate() {
        for second in &loads[position + 1..] {
            if first.destination != second.destination
                || first.width != second.width
                || !same_absolute_pool_value(output, *first, *second)
            {
                continue;
            }
            let between = &output.instructions[first.low + 1..second.high];
            if between.iter().any(|instruction| {
                instruction.float_destination() == Some(first.destination)
                    || is_control_flow(instruction)
            }) || has_alternate_entry(&output.instructions, first.high + 1..second.low + 1)
            {
                continue;
            }
            return Some((second.high, second.low));
        }
    }
    None
}

fn absolute_pool_load(
    output: &mwcc_machine_code::MachineFunction,
    high: usize,
) -> Option<AbsolutePoolLoad> {
    let Instruction::AddImmediateShifted {
        d: base,
        a: 0,
        immediate: 0,
    } = output.instructions.get(high)?
    else {
        return None;
    };
    let high_relocation = output
        .relocations
        .iter()
        .find(|relocation| relocation.instruction_index == high)?;
    if high_relocation.kind != RelocationKind::Addr16Ha {
        return None;
    }
    let search_end = (high + 9).min(output.instructions.len());
    for low in high + 1..search_end {
        let load = &output.instructions[low];
        let loaded = match load {
            Instruction::LoadFloatSingle { d, a, offset: 0 } if a == base => {
                Some((*d, PoolFloatWidth::Single))
            }
            Instruction::LoadFloatDouble { d, a, offset: 0 } if a == base => {
                Some((*d, PoolFloatWidth::Double))
            }
            _ => None,
        };
        if let Some((destination, width)) = loaded {
            let low_relocation = output
                .relocations
                .iter()
                .find(|relocation| relocation.instruction_index == low)?;
            if low_relocation.kind == RelocationKind::Addr16Lo
                && schedule_relocations::same_target_value(
                    &output.relocations,
                    &output.constants,
                    high,
                    low,
                )
                && !has_alternate_entry(&output.instructions, high + 1..low + 1)
            {
                return Some(AbsolutePoolLoad {
                    high,
                    low,
                    destination,
                    width,
                });
            }
            return None;
        }
        if is_control_flow(load) || writes_general_register(load, *base) {
            return None;
        }
    }
    None
}

fn writes_general_register(instruction: &Instruction, register: u8) -> bool {
    mwcc_vreg::register_operands(instruction)
        .iter()
        .any(|operand| {
            operand.role == mwcc_vreg::RegisterRole::Define
                && operand.class == mwcc_vreg::Class::General
                && operand.register == register
        })
}

fn same_absolute_pool_value(
    output: &mwcc_machine_code::MachineFunction,
    first: AbsolutePoolLoad,
    second: AbsolutePoolLoad,
) -> bool {
    schedule_relocations::same_relocated_value(
        &output.relocations,
        &output.constants,
        first.high,
        second.high,
    ) && schedule_relocations::same_relocated_value(
        &output.relocations,
        &output.constants,
        first.low,
        second.low,
    )
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
    use mwcc_machine_code::{PoolConstant, Relocation, RelocationTarget};

    fn repeated_loads(middle: Instruction) -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 3,
                    offset: 0,
                },
                middle,
                Instruction::AddImmediateShifted {
                    d: 4,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 4,
                    offset: 0,
                },
            ],
            relocations: vec![
                Relocation {
                    instruction_index: 0,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::Constant(0),
                },
                Relocation {
                    instruction_index: 1,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::Constant(0),
                },
                Relocation {
                    instruction_index: 3,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::Constant(0),
                },
                Relocation {
                    instruction_index: 4,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::Constant(0),
                },
            ],
            constants: vec![PoolConstant {
                bits: 4.0f32.to_bits().into(),
                byte_width: 4,
                static_slot: false,
                image: false,
                force_new: false,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_a_live_literal_reloaded_in_one_straight_line_region() {
        let output = repeated_loads(Instruction::LoadFloatSingle {
            d: 0,
            a: 31,
            offset: 20,
        });
        assert_eq!(redundant_absolute_pool_load(&output), Some((3, 4)));
    }

    #[test]
    fn recognizes_a_pool_load_with_scheduled_work_between_its_halves() {
        let mut output = repeated_loads(Instruction::LoadFloatSingle {
            d: 0,
            a: 31,
            offset: 20,
        });
        output.instructions.insert(
            1,
            Instruction::StoreHalfword {
                s: 5,
                a: 3,
                offset: 0,
            },
        );
        for relocation in &mut output.relocations {
            if relocation.instruction_index >= 1 {
                relocation.instruction_index += 1;
            }
        }
        assert_eq!(redundant_absolute_pool_load(&output), Some((4, 5)));
    }

    #[test]
    fn preserves_a_reload_after_its_fpr_is_overwritten() {
        let output = repeated_loads(Instruction::FloatMove { d: 1, b: 0 });
        assert_eq!(redundant_absolute_pool_load(&output), None);
    }

    #[test]
    fn preserves_a_reload_across_a_call() {
        let output = repeated_loads(Instruction::BranchAndLink {
            target: "callee".to_string(),
        });
        assert_eq!(redundant_absolute_pool_load(&output), None);
    }

    #[test]
    fn preserves_a_reload_reachable_without_the_first_load() {
        let mut output = repeated_loads(Instruction::LoadFloatSingle {
            d: 0,
            a: 31,
            offset: 20,
        });
        output.instructions.insert(
            0,
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
        );
        for relocation in &mut output.relocations {
            relocation.instruction_index += 1;
        }
        assert_eq!(redundant_absolute_pool_load(&output), None);
    }
}
