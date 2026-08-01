//! Entry-ready argument scheduling for normalized linkage-first frames.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fill build 163's three linkage latency slots after physical allocation.
    /// Allocator-owned callee-saved bodies cannot use the ordinary pre-allocation
    /// call-prologue scheduler, so their final machine stream is normalized here.
    pub(crate) fn schedule_linkage_first_entry_arguments(&mut self, physical_saved: &[u8]) {
        if self.schedule_linkage_first_asm_barrier_entry(physical_saved) {
            return;
        }
        let function_symbols = &self.call_return_types;
        let is_function_symbol = |name: &str| function_symbols.contains_key(name);
        schedule_guarded_saved_entry_copies(&mut self.output);
        schedule_entry_arguments(&mut self.output, &is_function_symbol);
        if let Some((from, to)) = schedule_entry_zero_store(&mut self.output) {
            self.labels.moved_before(from, to);
        }
        schedule_entry_wide_mask(&mut self.output);
    }

    /// Schedule a relocatable function-address pair in any linkage-first body.
    /// This narrow pass is safe even when the body has control flow because it
    /// only swaps the stack update with the immediately following address low.
    pub(crate) fn schedule_linkage_first_function_address(&mut self) {
        let function_symbols = &self.call_return_types;
        schedule_function_address_low(&mut self.output, &|name| {
            function_symbols.contains_key(name)
        });
    }

    /// Fill the first linkage slot for the compact eager/deferred inline
    /// frame. This stream has forward assertion branches, so the ordinary
    /// branch-free entry scheduler declines it; the retained-lane shape gives
    /// us a narrower proof and lets the label owner track the move safely.
    pub(crate) fn schedule_retained_eager_entry_argument(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some((from, to)) = retained_eager_entry_argument_move(&self.output) else {
            return;
        };
        if let Instruction::Or { a: 3, s, b } = self.output.instructions[from - 1] {
            if s == b {
                self.output.instructions[from - 1] = Instruction::AddImmediate {
                    d: 3,
                    a: s,
                    immediate: 0,
                };
            }
        }
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        remap_relocations_for_move(&mut self.output.relocations, from, to);
    }
}

/// A short-circuit guard consumes its saved scalar parameters before the first
/// call can clobber their incoming registers. Build 163 completes the physical
/// save range first, then copies the entry values from low saved register to
/// high; direct-call entry groups retain their separate latency-slot policy.
fn schedule_guarded_saved_entry_copies(
    output: &mut mwcc_machine_code::MachineFunction,
) {
    let Some(stack_update) = output.instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
        )
    }) else {
        return;
    };
    let Some(first_branch) = output.instructions[stack_update + 1..]
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
            )
        })
        .map(|offset| stack_update + 1 + offset)
    else {
        return;
    };
    for start in stack_update + 1..first_branch.saturating_sub(3) {
        let [first_store, first_copy, second_store, second_copy] =
            &output.instructions[start..start + 4]
        else {
            unreachable!()
        };
        let (
            Instruction::StoreWord {
                s: first_saved,
                a: 1,
                ..
            },
            Instruction::Or {
                a: first_home,
                s: first_incoming,
                b: first_again,
            },
            Instruction::StoreWord {
                s: second_saved,
                a: 1,
                ..
            },
            Instruction::Or {
                a: second_home,
                s: second_incoming,
                b: second_again,
            },
        ) = (first_store, first_copy, second_store, second_copy)
        else {
            continue;
        };
        if first_saved != first_home
            || second_saved != second_home
            || first_incoming != first_again
            || second_incoming != second_again
            || first_saved <= second_saved
        {
            continue;
        }
        let guard_consumes_saved_home = output.instructions[start + 4..=first_branch]
            .iter()
            .any(|instruction| {
                touches_general_register(instruction, *first_home)
                    || touches_general_register(instruction, *second_home)
            });
        if !guard_consumes_saved_home {
            output.instructions[start + 1] = Instruction::AddImmediate {
                d: *first_home,
                a: *first_incoming,
                immediate: 0,
            };
            return;
        }
        let reordered = [
            first_store.clone(),
            second_store.clone(),
            second_copy.clone(),
            first_copy.clone(),
        ];
        output.instructions[start..start + 4].clone_from_slice(&reordered);
        return;
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
            let [
                Instruction::StoreWord { s: first_saved, a: 1, .. },
                Instruction::StoreWord { s: second_saved, a: 1, .. },
                Instruction::LoadWord { d: eager, a: 3, .. },
                copy,
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] = window else {
                return false;
            };
            let copied = match copy {
                Instruction::AddImmediate { d: 3, a, immediate: 0 } => Some(*a),
                Instruction::Or { a: 3, s, b } if s == b => Some(*s),
                _ => None,
            };
            first_saved != second_saved
                && copied == Some(*eager)
                && eager == second_saved
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
fn schedule_entry_zero_store(
    output: &mut mwcc_machine_code::MachineFunction,
) -> Option<(usize, usize)> {
    let Some(stack_update) = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
    }) else {
        return None;
    };
    let Some(first_call) = output
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
    else {
        return None;
    };
    let Some(zero) = (stack_update + 1..first_call).find(|&index| {
        matches!(output.instructions[index],
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
            && matches!(output.instructions.get(index + 1),
                Some(Instruction::StoreWord { s: 0, a, .. }) if *a != 1)
    }) else {
        return None;
    };
    if output.instructions[stack_update..zero]
        .iter()
        .any(|instruction| {
            matches!(
                instruction,
                Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
            ) || touches_general_register(instruction, 0)
        })
    {
        return None;
    }
    let instruction = output.instructions.remove(zero);
    output.instructions.insert(stack_update, instruction);
    remap_relocations_for_move(&mut output.relocations, zero, stack_update);
    remap_branch_targets_for_move(&mut output.instructions, zero, stack_update);
    Some((zero, stack_update))
}

fn schedule_entry_arguments(
    output: &mut mwcc_machine_code::MachineFunction,
    is_function_symbol: &dyn Fn(&str) -> bool,
) {
    // Moving instructions changes instruction-index branch targets. Structured
    // control flow before the first call is deliberately left to its semantic
    // owner. A branch after that call cannot be crossed by these prologue-only
    // moves and leaves every branch instruction and target at the same index.
    let first_call = output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { .. })
    });
    let first_control = output.instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
        )
    });
    if matches!((first_call, first_control), (Some(call), Some(control)) if control < call) {
        return;
    }

    schedule_function_address_low(output, is_function_symbol);

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
fn schedule_function_address_low(
    output: &mut mwcc_machine_code::MachineFunction,
    is_function_symbol: &dyn Fn(&str) -> bool,
) {
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
        if !is_function_symbol(target) {
            return None;
        }
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
    // A register match is not a symbol-pair proof. Another function address
    // may already own this entry `lis`, while the candidate callback's real
    // high half lives in a later control-flow arm. Attaching both relocations
    // to the entry instruction would form neither address correctly.
    if output.relocations.iter().any(|relocation| {
        relocation.instruction_index == high
            && relocation.kind == RelocationKind::Addr16Ha
            && !matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(name) if name == &target
            )
    }) {
        return;
    }
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

fn remap_branch_targets_for_move(instructions: &mut [Instruction], from: usize, to: usize) {
    for instruction in instructions {
        let target = match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => target,
            _ => continue,
        };
        *target = if *target == from {
            to
        } else if (to..from).contains(target) {
            *target + 1
        } else {
            *target
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    #[test]
    fn guarded_saved_copies_follow_the_complete_save_range() {
        let mut output = mwcc_machine_code::MachineFunction::new("guarded");
        output.instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::move_register(31, 5),
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::move_register(30, 3),
            Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 9,
            },
        ];

        schedule_guarded_saved_entry_copies(&mut output);

        assert!(matches!(
            &output.instructions[1..5],
            [
                Instruction::StoreWord { s: 31, .. },
                Instruction::StoreWord { s: 30, .. },
                Instruction::Or { a: 30, s: 3, b: 3 },
                Instruction::Or { a: 31, s: 5, b: 5 },
            ]
        ));
    }

    #[test]
    fn later_only_saved_parameters_keep_interleaved_materialized_entry_copies() {
        let mut output = mwcc_machine_code::MachineFunction::new("guarded");
        output.instructions = vec![
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
            Instruction::move_register(31, 4),
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 16,
            },
            Instruction::move_register(30, 3),
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 8352,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            },
        ];
        schedule_guarded_saved_entry_copies(&mut output);

        assert!(matches!(
            &output.instructions[1..5],
            [
                Instruction::StoreWord { s: 31, .. },
                Instruction::AddImmediate {
                    d: 31,
                    a: 4,
                    immediate: 0
                },
                Instruction::StoreWord { s: 30, .. },
                Instruction::Or { a: 30, s: 3, b: 3 },
            ]
        ));
    }

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

        schedule_entry_arguments(&mut output, &|_| true);

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
    fn fills_literal_linkage_slots_before_later_control_flow() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::load_immediate(4, 128),
            Instruction::load_immediate(5, 0),
            Instruction::BranchAndLink { target: "ack".into() },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 10,
            },
        ];

        schedule_entry_arguments(&mut output, &|_| true);

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediate { d: 4, a: 0, immediate: 128 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
                Instruction::StoreWord { s: 31, a: 1, offset: 20 },
                Instruction::BranchAndLink { .. },
                ..
            ]
        ));
        assert!(matches!(
            output.instructions[9],
            Instruction::BranchConditionalForward { target: 10, .. }
        ));
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

        schedule_entry_arguments(&mut output, &|_| true);

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
    fn leaves_a_non_function_address_low_after_the_stack_update() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 3,
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
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "consume".to_string(),
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("buffer".to_string()),
            },
            Relocation {
                instruction_index: 4,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("buffer".to_string()),
            },
        ];

        schedule_entry_arguments(&mut output, &|name| name == "callback");

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::BranchAndLink { .. },
            ]
        ));
        assert_eq!(output.relocations[1].instruction_index, 4);
    }

    #[test]
    fn does_not_pair_a_later_function_low_with_an_unrelated_entry_high() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 4,
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
                d: 0,
                a: 4,
                immediate: 0,
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
            Instruction::BranchAndLink {
                target: "install".to_string(),
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("entry".to_string()),
            },
            Relocation {
                instruction_index: 4,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("entry".to_string()),
            },
            Relocation {
                instruction_index: 5,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".to_string()),
            },
            Relocation {
                instruction_index: 6,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("callback".to_string()),
            },
        ];

        schedule_function_address_low(&mut output, &|_| true);

        assert!(matches!(
            output.instructions[3],
            Instruction::StoreWordWithUpdate { .. }
        ));
        assert_eq!(output.relocations[0].instruction_index, 1);
        assert_eq!(output.relocations[2].instruction_index, 5);
        assert_eq!(output.relocations[3].instruction_index, 6);
    }

    #[test]
    fn schedules_an_entry_zero_despite_later_control_flow() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 31, a: 3, offset: 44 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 6528 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 10,
            },
            Instruction::BranchAndLink { target: "first".into() },
            Instruction::BranchAndLink { target: "second".into() },
        ];

        assert_eq!(schedule_entry_zero_store(&mut output), Some((5, 2)));
        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: 31, .. },
                Instruction::LoadWord { d: 31, .. },
                Instruction::StoreWord { s: 0, a: 31, offset: 6528 },
                ..
            ]
        ));
        assert!(matches!(
            output.instructions[8],
            Instruction::BranchConditionalForward { target: 10, .. }
        ));
    }

    #[test]
    fn preserves_an_entry_branch_target_crossed_by_the_zero_move() {
        let mut output = mwcc_machine_code::MachineFunction::new("test");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 31, a: 3, offset: 44 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 6528 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 4,
            },
            Instruction::BranchAndLink { target: "first".into() },
        ];

        assert_eq!(schedule_entry_zero_store(&mut output), Some((5, 2)));
        assert!(matches!(
            output.instructions[7],
            Instruction::BranchConditionalForward { target: 5, .. }
        ));
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
