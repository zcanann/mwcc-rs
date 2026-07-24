//! Entry-ready argument scheduling for normalized linkage-first frames.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fill build 163's three linkage latency slots after physical allocation.
    /// Allocator-owned callee-saved bodies cannot use the ordinary pre-allocation
    /// call-prologue scheduler, so their final machine stream is normalized here.
    pub(crate) fn schedule_linkage_first_entry_arguments(&mut self) {
        schedule_entry_arguments(&mut self.output);
        schedule_entry_zero_store(&mut self.output);
        schedule_entry_wide_mask(&mut self.output);
    }

    /// Schedule a relocatable function-address pair in any linkage-first body.
    /// This narrow pass is safe even when the body has control flow because it
    /// only swaps the stack update with the immediately following address low.
    pub(crate) fn schedule_linkage_first_function_address(&mut self) {
        schedule_function_address_low(&mut self.output);
    }

    /// Fill the first linkage slot for the compact eager/deferred inline
    /// frame. This stream has forward assertion branches, so the ordinary
    /// branch-free entry scheduler declines it; the retained-lane shape gives
    /// us a narrower proof and lets the label owner track the move safely.
    pub(crate) fn schedule_retained_eager_entry_argument(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_callee_saved_frame_layout
                != LegacyCalleeSavedFrameLayout::RetainEagerLocalLane
        {
            return;
        }
        let Some((from, to)) = retained_eager_entry_argument_move(&self.output) else {
            return;
        };
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        remap_relocations_for_move(&mut self.output.relocations, from, to);
    }
}

fn retained_eager_entry_argument_move(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<(usize, usize)> {
    let link_read = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 })
    })?;
    let link_store = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset: 4 })
    })?;
    let stack_update = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
    })?;
    if !(link_read < link_store && link_store < stack_update) {
        return None;
    }
    let first_call = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { .. })
    })?;
    let from = output.instructions[stack_update + 1..=first_call]
        .windows(6)
        .position(|window| {
            matches!(window, [
                Instruction::StoreWord { s: first_saved, a: 1, .. },
                Instruction::StoreWord { s: second_saved, a: 1, .. },
                Instruction::LoadWord { d: eager, a: 3, .. },
                Instruction::AddImmediate { d: 3, a: copied, immediate: 0 },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if first_saved != second_saved && eager == copied && eager == second_saved)
        })?
        + stack_update
        + 1
        + 4;
    if output
        .relocations
        .iter()
        .any(|relocation| relocation.instruction_index == from)
    {
        return None;
    }
    Some((from, link_read + 1))
}

/// A two-instruction discontiguous mask is ready at entry, but its low half
/// writes r0 and therefore must wait until after the saved-LR store.  MWCC puts
/// the independent high half in the first linkage slot and the dependent low
/// half immediately before `stwu`.
fn schedule_entry_wide_mask(output: &mut mwcc_machine_code::MachineFunction) {
    let Some(link_read) = output.instructions.iter().position(
        |instruction| matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 }),
    ) else {
        return;
    };
    let Some(link_store) = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset: 4 })
    }) else {
        return;
    };
    let Some(stack_update) = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
    }) else {
        return;
    };
    if !(link_read < link_store && link_store < stack_update) {
        return;
    }

    let candidate = (stack_update + 1..output.instructions.len().saturating_sub(3)).find_map(
        |high| {
            let Instruction::AddImmediateShifted {
                d: high_register,
                a: 0,
                ..
            } = output.instructions[high]
            else {
                return None;
            };
            let [
                Instruction::AddImmediate {
                    d: 0,
                    a: low_base,
                    ..
                },
                Instruction::LoadWord { d: value, .. },
                Instruction::AndRecord { a: 0, s, b: 0 },
            ] = output.instructions.get(high + 1..high + 4)?
            else {
                return None;
            };
            (high_register != 0 && *low_base == high_register && value == s)
                .then_some((high, high + 1, high_register))
        },
    );
    let Some((high, low, high_register)) = candidate else {
        return;
    };
    if output.relocations.iter().any(|relocation| {
        relocation.instruction_index == high || relocation.instruction_index == low
    }) || output.instructions[link_read + 1..high]
        .iter()
        .any(|instruction| touches_general_register(instruction, high_register))
    {
        return;
    }

    let high_instruction = output.instructions.remove(high);
    output.instructions.insert(link_read + 1, high_instruction);
    remap_relocations_for_move(&mut output.relocations, high, link_read + 1);

    // Moving the high half earlier leaves the low half at the same index: one
    // removal before it and one insertion before it cancel out.
    let low_instruction = output.instructions.remove(low);
    let stack_update = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
    }).expect("the recognized stack update remains present");
    output.instructions.insert(stack_update, low_instruction);
    remap_relocations_for_move(&mut output.relocations, low, stack_update);
}

/// A scratch zero feeding the first body store cannot fill the dependency slot
/// immediately after `mflr`, but it is independent of the stack update. MWCC
/// places it between the LR store and `stwu` in this retained-receiver shape.
fn schedule_entry_zero_store(output: &mut mwcc_machine_code::MachineFunction) {
    if output.instructions.iter().any(|instruction| {
        matches!(instruction, Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. })
    }) {
        return;
    }
    let Some(stack_update) = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
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
    let Some(zero) = (stack_update + 1..first_call).find(|&index| {
        matches!(output.instructions[index],
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
            && matches!(output.instructions.get(index + 1),
                Some(Instruction::StoreWord { s: 0, a, .. }) if *a != 1)
    }) else {
        return;
    };
    let instruction = output.instructions.remove(zero);
    output.instructions.insert(stack_update, instruction);
    remap_relocations_for_move(&mut output.relocations, zero, stack_update);
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

    #[test]
    fn splits_a_wide_mask_across_the_linkage_slots() {
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
            Instruction::load_immediate(31, 0),
            Instruction::load_immediate_shifted(4, -32768),
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 0x0f00,
            },
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 1640,
            },
            Instruction::AndRecord { a: 0, s: 5, b: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
        ];

        schedule_entry_wide_mask(&mut output);

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                },
                Instruction::AddImmediate { d: 0, a: 4, .. },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: 31, .. },
                Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
                Instruction::LoadWord { d: 5, .. },
                Instruction::AndRecord { a: 0, s: 5, b: 0 },
                Instruction::BranchConditionalForward { .. },
            ]
        ));
    }

    #[test]
    fn finds_retained_eager_argument_across_later_assertion_branches() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::LoadWord { d: 30, a: 3, offset: 44 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::load_immediate(4, 4),
            Instruction::BranchAndLink { target: "lookup".into() },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 11,
            },
            Instruction::BranchAndLink { target: "__assert".into() },
        ];

        assert_eq!(retained_eager_entry_argument_move(&output), Some((7, 1)));
    }
}
