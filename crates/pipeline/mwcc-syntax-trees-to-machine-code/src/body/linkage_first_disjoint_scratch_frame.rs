//! Pack disjoint addressable-local lifetimes in a linkage-first scratch frame.
//!
//! Build 163 overlays a four-byte call result scratch with the unused prefix
//! of a later twelve-byte aggregate. Independent local allocation gives the
//! aggregate the low frame address and places the scalar after it, growing the
//! frame by one alignment lane. This pass verifies the complete physical
//! lifetime boundary before exchanging their homes and compacting the frame.

use super::*;
use mwcc_machine_code::RelocationTarget;

const UNPACKED_FRAME_SIZE: i16 = 32;
const PACKED_FRAME_SIZE: i16 = 24;
const UNPACKED_AGGREGATE_OFFSET: i16 = 8;
const UNPACKED_SCALAR_OFFSET: i16 = 12;
const PACKED_SCALAR_OFFSET: i16 = 8;
const PACKED_AGGREGATE_OFFSET: i16 = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    link_store: usize,
    scalar_address: usize,
    scalar_entry_branches: Vec<usize>,
    scalar_reload: usize,
    aggregate_addresses: [usize; 2],
    epilogue_restore: usize,
}

fn external_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(target) => Some(target.as_str()),
            _ => None,
        }
    })
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Plan> {
    let instructions = &output.instructions;
    if instructions.len() < 13
        || !matches!(
            instructions.get(0..4),
            Some([
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32,
                },
                Instruction::MoveFromLinkRegister { d: 0 },
                _,
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 36,
                },
            ])
        )
    {
        return None;
    }

    let epilogue_restore = instructions.len() - 4;
    if !matches!(
        instructions.get(epilogue_restore..),
        Some([
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ])
    ) {
        return None;
    }

    let first_call = instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))?;
    let scalar_address = first_call.checked_sub(4)?;
    let [
        Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 12,
        },
        Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            ..
        },
        Instruction::AddImmediate { d: 4, a: 4, .. },
        Instruction::LoadWord { d: 4, a: 4, .. },
        Instruction::BranchAndLink { .. },
    ] = &instructions[scalar_address..=first_call]
    else {
        return None;
    };
    let address_target = external_target(output, scalar_address + 1, RelocationKind::Addr16Ha)?;
    if external_target(output, scalar_address + 2, RelocationKind::Addr16Lo)
        != Some(address_target)
    {
        return None;
    }
    let scalar_entry_branches: Vec<usize> = instructions[..scalar_address]
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. }
                if *target == scalar_address =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    if scalar_entry_branches.is_empty() {
        return None;
    }

    let scalar_reload = first_call + 1;
    if !matches!(
        instructions.get(scalar_reload),
        Some(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        })
    ) {
        return None;
    }

    let aggregate_addresses: Vec<usize> = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 3,
                    a: 1,
                    immediate: 8,
                }
            )
            .then_some(index)
        })
        .collect();
    let [first_aggregate, second_aggregate] = aggregate_addresses.as_slice() else {
        return None;
    };
    if *first_aggregate <= scalar_reload
        || !matches!(
            instructions.get(first_aggregate + 1),
            Some(Instruction::BranchAndLink { .. })
        )
        || *second_aggregate != first_aggregate + 2
        || !matches!(
            instructions.get(second_aggregate + 1),
            Some(Instruction::BranchAndLink { .. })
        )
    {
        return None;
    }

    let allowed_stack_owners = [
        0,
        3,
        scalar_address,
        scalar_reload,
        *first_aggregate,
        *second_aggregate,
        epilogue_restore,
        epilogue_restore + 1,
    ];
    if instructions.iter().enumerate().any(|(index, instruction)| {
        !allowed_stack_owners.contains(&index)
            && mwcc_vreg::register_operands(instruction)
                .iter()
                .any(|operand| operand.register == 1)
    }) {
        return None;
    }

    Some(Plan {
        link_store: 3,
        scalar_address,
        scalar_entry_branches,
        scalar_reload,
        aggregate_addresses: [*first_aggregate, *second_aggregate],
        epilogue_restore,
    })
}

impl Generator {
    pub(crate) fn pack_linkage_first_disjoint_scratch_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || !self.callee_saved.is_empty()
            || self.callee_saved_float != 0
            || self.frame_size != UNPACKED_FRAME_SIZE
            || self.frame_slots.len() != 2
        {
            return;
        }
        let has_scalar = self.frame_slots.values().any(|slot| {
            slot.offset == UNPACKED_SCALAR_OFFSET
                && slot.size == 4
                && !slot.is_array
                && !matches!(slot.value_type, Type::Struct { .. })
        });
        let has_aggregate = self.frame_slots.values().any(|slot| {
            slot.offset == UNPACKED_AGGREGATE_OFFSET
                && slot.size == 12
                && !slot.is_array
                && matches!(slot.value_type, Type::Struct { size: 12, .. })
        });
        if !has_scalar || !has_aggregate {
            return;
        }
        let Some(plan) = recognize(&self.output) else {
            return;
        };

        let Instruction::StoreWordWithUpdate { offset, .. } =
            &mut self.output.instructions[0]
        else {
            unreachable!("the scratch-frame plan owns its frame update")
        };
        *offset = -PACKED_FRAME_SIZE;
        let Instruction::StoreWord { offset, .. } =
            &mut self.output.instructions[plan.link_store]
        else {
            unreachable!("the scratch-frame plan owns its link store")
        };
        *offset = 4;
        let Instruction::AddImmediate { immediate, .. } =
            &mut self.output.instructions[plan.scalar_address]
        else {
            unreachable!("the scratch-frame plan owns its scalar address")
        };
        *immediate = PACKED_SCALAR_OFFSET;
        let Instruction::LoadWord { offset, .. } =
            &mut self.output.instructions[plan.scalar_reload]
        else {
            unreachable!("the scratch-frame plan owns its scalar reload")
        };
        *offset = PACKED_SCALAR_OFFSET;
        for address in plan.aggregate_addresses {
            let Instruction::AddImmediate { immediate, .. } =
                &mut self.output.instructions[address]
            else {
                unreachable!("the scratch-frame plan owns its aggregate addresses")
            };
            *immediate = PACKED_AGGREGATE_OFFSET;
        }
        let Instruction::AddImmediate { immediate, .. } =
            &mut self.output.instructions[plan.epilogue_restore]
        else {
            unreachable!("the scratch-frame plan owns its epilogue")
        };
        *immediate = PACKED_FRAME_SIZE;

        for slot in self.frame_slots.values_mut() {
            if slot.offset == UNPACKED_SCALAR_OFFSET && slot.size == 4 {
                slot.offset = PACKED_SCALAR_OFFSET;
            } else if slot.offset == UNPACKED_AGGREGATE_OFFSET && slot.size == 12 {
                slot.offset = PACKED_AGGREGATE_OFFSET;
            }
        }
        self.frame_size = PACKED_FRAME_SIZE;

        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, plan.link_store, 1);
        crate::move_instruction_before_retargeting(
            self,
            plan.scalar_address + 1,
            plan.scalar_address,
        );
        // The address-high instruction becomes the shared switch-arm entry,
        // rather than merely executing on fallthrough. The generic move helper
        // deliberately preserves the old scalar-address identity, so restore
        // this join-specific target ownership explicitly.
        for branch in plan.scalar_entry_branches {
            match &mut self.output.instructions[branch] {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => {
                    *target = plan.scalar_address;
                }
                _ => unreachable!("the scratch-frame plan recorded an entry branch"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation, RelocationTarget};

    fn call(target: &str) -> Instruction {
        Instruction::BranchAndLink {
            target: target.into(),
        }
    }

    fn candidate() -> MachineFunction {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
        ];
        instructions.extend((0..18).map(|_| Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        }));
        instructions[17] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 22,
        };
        instructions[20] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 22,
        };
        instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 12,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 128,
            },
            call("read"),
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            call("construct"),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            call("post"),
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        MachineFunction {
            instructions,
            relocations: vec![
                Relocation {
                    instruction_index: 23,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::External("state".into()),
                },
                Relocation {
                    instruction_index: 24,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::External("state".into()),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_disjoint_scalar_and_aggregate_call_lifetimes() {
        assert_eq!(
            recognize(&candidate()),
            Some(Plan {
                link_store: 3,
                scalar_address: 22,
                scalar_entry_branches: vec![17, 20],
                scalar_reload: 27,
                aggregate_addresses: [29, 31],
                epilogue_restore: 33,
            })
        );
    }

    #[test]
    fn rejects_an_additional_stack_use_between_the_call_lifetimes() {
        let mut output = candidate();
        output.instructions[28] = Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 16,
        };

        assert_eq!(recognize(&output), None);
    }
}
