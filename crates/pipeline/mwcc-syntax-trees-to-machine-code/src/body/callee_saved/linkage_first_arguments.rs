//! Entry-ready argument scheduling for normalized linkage-first frames.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fill build 163's three linkage latency slots after physical allocation.
    /// Allocator-owned callee-saved bodies cannot use the ordinary pre-allocation
    /// call-prologue scheduler, so their final machine stream is normalized here.
    pub(crate) fn schedule_linkage_first_entry_arguments(&mut self) {
        schedule_entry_arguments(&mut self.output);
    }

    /// Schedule a relocatable function-address pair in any linkage-first body.
    /// This narrow pass is safe even when the body has control flow because it
    /// only swaps the stack update with the immediately following address low.
    pub(crate) fn schedule_linkage_first_function_address(&mut self) {
        schedule_function_address_low(&mut self.output);
    }
}

fn schedule_entry_arguments(output: &mut mwcc_machine_code::MachineFunction) {
    // Moving instructions changes instruction-index branch targets. Structured
    // control flow is deliberately left to its semantic owner until this pass
    // also has a branch-target remapper.
    if output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
        )
    }) {
        return;
    }

    schedule_function_address_low(output);

    for slot in 0..3 {
        let Some(link_read) = output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 })
        }) else {
            return;
        };
        let Some(link_store) = output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                }
            )
        }) else {
            return;
        };
        let Some(stack_update) = output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
            )
        }) else {
            return;
        };
        let Some(first_call) = output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };
        if !(link_read < link_store && link_store < stack_update && stack_update < first_call) {
            return;
        }

        let insertion = if slot == 0 {
            link_read + 1
        } else {
            stack_update
        };
        let candidate = (stack_update + 1..first_call).find(|&index| {
            let register = match output.instructions[index] {
                Instruction::AddImmediate { d, a: 0, .. } if (3..=10).contains(&d) => d,
                _ => return false,
            };
            if output
                .relocations
                .iter()
                .any(|relocation| relocation.instruction_index == index)
            {
                return false;
            }
            !output.instructions[insertion..index]
                .iter()
                .chain(&output.instructions[index + 1..first_call])
                .any(|instruction| touches_general_register(instruction, register))
        });
        let Some(candidate) = candidate else { return };

        let instruction = output.instructions.remove(candidate);
        output.instructions.insert(insertion, instruction);
        remap_relocations_for_move(&mut output.relocations, candidate, insertion);
    }
}

/// A function address is a dependent `lis @ha; addi @l` pair. Frame
/// normalization already leaves the `lis` in the first linkage slot; move its
/// `addi` from after `stwu` into the second slot, preserving both relocations.
fn schedule_function_address_low(output: &mut mwcc_machine_code::MachineFunction) {
    let Some(link_read) = output
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 }))
    else {
        return;
    };
    let Some(link_store) = output.instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4
            }
        )
    }) else {
        return;
    };
    let Some(stack_update) = output.instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
        )
    }) else {
        return;
    };
    let Some(first_call) = output
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
    else {
        return;
    };
    if !(link_read < link_store && link_store < stack_update && stack_update < first_call) {
        return;
    }

    let low = (stack_update + 1..first_call).find_map(|index| {
        let Instruction::AddImmediate { d, a, .. } = output.instructions[index] else {
            return None;
        };
        if d != a {
            return None;
        }
        let relocation = output.relocations.iter().find(|relocation| {
            relocation.instruction_index == index && relocation.kind == RelocationKind::Addr16Lo
        })?;
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some((index, d, target.clone()))
    });
    let Some((low, register, target)) = low else {
        return;
    };
    let Some(high) = (link_read + 1..link_store).find(|&index| {
        matches!(output.instructions[index],
            Instruction::AddImmediateShifted { d, a: 0, .. } if d == register)
    }) else {
        return;
    };
    // Some hand-owned frame normalizers rotate this prefix before their
    // relocation remap. Re-anchor the matching @ha relocation to its lis.
    let Some(high_relocation) = output.relocations.iter_mut().find(|relocation| {
        relocation.kind == RelocationKind::Addr16Ha
            && matches!(&relocation.target,
                mwcc_machine_code::RelocationTarget::External(name) if name == &target)
    }) else {
        return;
    };
    high_relocation.instruction_index = high;
    let instruction = output.instructions.remove(low);
    output.instructions.insert(stack_update, instruction);
    remap_relocations_for_move(&mut output.relocations, low, stack_update);
}

fn touches_general_register(instruction: &Instruction, register: u8) -> bool {
    mwcc_vreg::register_operands(instruction)
        .into_iter()
        .any(|operand| operand.class == mwcc_vreg::Class::General && operand.register == register)
}

fn remap_relocations_for_move(
    relocations: &mut [mwcc_machine_code::Relocation],
    from: usize,
    to: usize,
) {
    debug_assert!(to < from);
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == from => to,
            index if (to..from).contains(&index) => index + 1,
            index => index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    #[test]
    fn fills_three_linkage_slots_and_tracks_crossed_relocation() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 289,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 144,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 2,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "mixed_sink".to_string(),
            },
        ];
        output.relocations.push(Relocation {
            instruction_index: 8,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External("@2".to_string()),
        });

        schedule_entry_arguments(&mut output);

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: 289
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 144
                },
                Instruction::AddImmediate {
                    d: 6,
                    a: 0,
                    immediate: 0
                },
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -24
                },
                ..
            ]
        ));
        assert!(matches!(
            output.instructions[9],
            Instruction::LoadFloatSingle {
                d: 1,
                a: 2,
                offset: 0
            }
        ));
        assert_eq!(output.relocations[0].instruction_index, 9);
    }

    #[test]
    fn places_function_address_low_before_the_stack_update() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "install".to_string(),
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".to_string()),
            },
            Relocation {
                instruction_index: 4,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("callback".to_string()),
            },
        ];

        schedule_entry_arguments(&mut output);

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediateShifted { d: 5, a: 0, .. },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                },
                Instruction::AddImmediate { d: 5, a: 5, .. },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::BranchAndLink { .. },
            ]
        ));
        assert_eq!(output.relocations[0].instruction_index, 1);
        assert_eq!(output.relocations[1].instruction_index, 3);
    }
}
