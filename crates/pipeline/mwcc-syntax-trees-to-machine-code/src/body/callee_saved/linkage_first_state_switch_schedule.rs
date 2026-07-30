//! Late linkage-first latency filling for state-machine switches.
//!
//! These schedules are independent of particular symbol names: a retained
//! receiver fills the last linkage slot, an absolute-address high half can
//! precede an unrelated small-data load, and a wide constant can fill the
//! volatile fixed-bank self-copy latency.

#[allow(unused_imports)]
use super::*;

fn state_switch_entry_receiver(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<(usize, usize)> {
    let stack_update = instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
        )
    })?;
    let [Instruction::StoreWord { s: 0, a: 0, .. }, Instruction::LoadWord { d: 0, a: 3, .. }, Instruction::Or {
        a: receiver,
        s: 3,
        b: 3,
    }] = instructions.get(stack_update + 1..stack_update + 4)?
    else {
        return None;
    };
    if !(4..=31).contains(receiver)
        || !relocations.iter().any(|relocation| {
            relocation.instruction_index == stack_update + 1
                && relocation.kind == RelocationKind::EmbSda21
        })
    {
        return None;
    }
    Some((stack_update + 3, stack_update))
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

fn global_address_high_after_small_data_load(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<usize> {
    relocations.iter().find_map(|high| {
        if high.kind != RelocationKind::Addr16Ha || high.instruction_index == 0 {
            return None;
        }
        let index = high.instruction_index;
        let Instruction::AddImmediateShifted {
            d: high_base, a: 0, ..
        } = instructions.get(index)?
        else {
            return None;
        };
        let Instruction::LoadWord { .. } = instructions.get(index - 1)? else {
            return None;
        };
        if !relocations.iter().any(|relocation| {
            relocation.instruction_index == index - 1 && relocation.kind == RelocationKind::EmbSda21
        }) || mwcc_vreg::register_operands(&instructions[index - 1])
            .into_iter()
            .any(|operand| {
                operand.class == mwcc_vreg::Class::General && operand.register == *high_base
            })
        {
            return None;
        }
        let target = external_target(high)?;
        let low = relocations.iter().find(|relocation| {
            relocation.instruction_index == index + 1
                && relocation.kind == RelocationKind::Addr16Lo
                && external_target(relocation) == Some(target)
        })?;
        if !matches!(
            instructions.get(low.instruction_index),
            Some(Instruction::AddImmediate { a, .. }) if a == high_base
        ) {
            return None;
        }
        let Some(Instruction::AddImmediate { d: low_value, .. }) =
            instructions.get(low.instruction_index)
        else {
            return None;
        };
        if !matches!(
            instructions.get(low.instruction_index + 1),
            Some(Instruction::StoreWord { s, a: 0, .. }) if s == low_value
        ) || !relocations.iter().any(|relocation| {
            relocation.instruction_index == low.instruction_index + 1
                && relocation.kind == RelocationKind::EmbSda21
        }) {
            return None;
        }
        Some(index)
    })
}

fn fixed_bank_following_wide_constant(instructions: &[Instruction]) -> Option<usize> {
    instructions
        .windows(6)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::AddImmediateShifted { d: bank, a: 0, .. }, Instruction::AddImmediate {
                d: completed,
                a: bank_source,
                immediate: low,
            }, Instruction::LoadWord {
                d: scratch,
                a: load_base,
                offset,
            }, Instruction::StoreWord {
                s,
                a: store_base,
                offset: store_offset,
            }, Instruction::LoadWord { .. }, Instruction::AddImmediateShifted {
                d: constant,
                a: 0,
                immediate,
            }] = window
            else {
                return None;
            };
            if bank != completed
                || bank != bank_source
                || bank != load_base
                || bank != store_base
                || scratch != s
                || offset != store_offset
                || *low == 0
                || *immediate == 0
                || window[3..5].iter().any(|instruction| {
                    mwcc_vreg::register_operands(instruction)
                        .into_iter()
                        .any(|operand| {
                            operand.class == mwcc_vreg::Class::General
                                && operand.register == *constant
                        })
                })
            {
                return None;
            }
            Some(start + 5)
        })
}

fn is_control_flow_target(generator: &Generator, index: usize) -> bool {
    generator.output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target }
                if *target == index
        )
    }) || generator.output.jump_tables.iter().any(|table| {
        table
            .entries
            .iter()
            .any(|entry| *entry as usize / 4 == index)
    })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_state_switch_layout(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        if let Some((from, to)) =
            state_switch_entry_receiver(&self.output.instructions, &self.output.relocations)
        {
            crate::move_instruction_before_retargeting(self, from, to);
        }

        while let Some(high) = global_address_high_after_small_data_load(
            &self.output.instructions,
            &self.output.relocations,
        ) {
            if is_control_flow_target(self, high - 1) {
                break;
            }
            crate::move_instruction_before_retargeting(self, high, high - 1);
        }

        if self.behavior.fixed_address_poll_address_style
            == mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            if let Some(constant) = fixed_bank_following_wide_constant(&self.output.instructions) {
                let insertion = constant - 2;
                if !(insertion..constant).any(|index| is_control_flow_target(self, index))
                    && !self
                        .output
                        .relocations
                        .iter()
                        .any(|relocation| relocation.instruction_index == constant)
                    && !self
                        .output
                        .data_section_displacements
                        .iter()
                        .any(|displacement| displacement.instruction_index == constant)
                {
                    crate::move_instruction_before_retargeting(self, constant, insertion);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.to_string()),
        }
    }

    #[test]
    fn recognizes_a_receiver_after_state_publication_and_dispatch_load() {
        let instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 8,
            },
            Instruction::move_register(7, 3),
        ];
        let relocations = vec![relocation(1, RelocationKind::EmbSda21, "state")];

        assert_eq!(
            state_switch_entry_receiver(&instructions, &relocations),
            Some((3, 0))
        );
    }

    #[test]
    fn recognizes_an_absolute_high_after_an_independent_small_data_load() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate_shifted(3, 0),
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
        ];
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "current"),
            relocation(1, RelocationKind::Addr16Ha, "replacement"),
            relocation(2, RelocationKind::Addr16Lo, "replacement"),
            relocation(3, RelocationKind::EmbSda21, "published"),
        ];

        assert_eq!(
            global_address_high_after_small_data_load(&instructions, &relocations),
            Some(1)
        );
    }

    #[test]
    fn recognizes_a_wide_constant_after_a_fixed_bank_self_copy() {
        let instructions = vec![
            Instruction::load_immediate_shifted(3, -13312),
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0x6000,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 3,
                a: 7,
                offset: 32,
            },
            Instruction::load_immediate_shifted(4, 8),
        ];

        assert_eq!(fixed_bank_following_wide_constant(&instructions), Some(5));
    }
}
