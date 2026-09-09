//! Schedule a leading fixed-address quotient through a linkage-first frame.
//!
//! This runs after frame normalization. A validated entry packet owns only the
//! linkage stores, saved GPRs, one fixed load, and its multiplier. An optional
//! section base can reuse the dead multiplier while the multiply completes.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, MachineFunction, RelocationKind, RelocationTarget};
use mwcc_versions::{FrameConvention, Optimization, OptimizationGoal};
use mwcc_vreg::{Class, Liveness};

impl Generator {
    pub(crate) fn schedule_fixed_address_division_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization < Optimization::O2
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.preceded_by_asm
        {
            return;
        }
        let anchor = self
            .data_section_anchor
            .as_ref()
            .and_then(|p| p.register.map(|_| p.anchor_symbol.as_str()));
        if let Some(plan) =
            entry_plan_with_calls(&self.output, self.frame_size, anchor, |name, register| {
                // Void scalar-argument calls have no hidden result pointer or
                // variadic argument lanes beyond the declared parameters.
                self.call_return_types.get(name) == Some(&mwcc_syntax_trees::Type::Void)
                    && !self.variadic_callees.contains(name)
                    && self
                        .call_parameter_types
                        .get(name)
                        .is_some_and(|parameters| {
                            parameters.iter().all(|ty| {
                                ty.width() <= 32
                                    && !matches!(
                                        ty,
                                        mwcc_syntax_trees::Type::Float
                                            | mwcc_syntax_trees::Type::Void
                                    )
                            }) && usize::from(register) >= 3 + parameters.len()
                        })
            })
        {
            plan.apply(&mut self.output);
        }
    }
}

#[derive(Debug)]
struct EntryPlan {
    order: Vec<usize>,
    anchor: Option<(usize, usize, u8)>,
}

impl EntryPlan {
    fn apply(self, output: &mut MachineFunction) {
        if let Some((high, low, register)) = self.anchor {
            if let Instruction::AddImmediateShifted { d, .. } = &mut output.instructions[high] {
                *d = register;
            }
            if let Instruction::AddImmediate { a, .. } = &mut output.instructions[low] {
                *a = register;
            }
        }
        crate::permute_machine_function_region(output, 0, &self.order);
    }
}

fn entry_plan_with_calls(
    output: &MachineFunction,
    frame_size: i16,
    anchor: Option<&str>,
    call_ignores: impl Fn(&str, u8) -> bool,
) -> Option<EntryPlan> {
    use Instruction::*;
    let instructions = &output.instructions;
    if frame_size <= 0
        || !matches!(instructions.first(), Some(MoveFromLinkRegister { d: 0 }))
        || !output.jump_tables.is_empty()
        || instructions
            .iter()
            .any(|i| matches!(i, VerbatimWord { .. } | BranchToCountRegister))
    {
        return None;
    }
    let multiply = instructions.iter().take(16).position(|i| {
        matches!(
            i,
            MultiplyHighWordUnsigned {
                d: 0,
                a: 3..=12,
                b: 0
            }
        )
    })?;
    let MultiplyHighWordUnsigned { a: multiplier, .. } = instructions[multiply] else {
        return None;
    };
    let end = multiply + 2;
    if !matches!(
        instructions.get(multiply + 1),
        Some(ShiftRightLogicalImmediate { s: 0, .. })
    ) {
        return None;
    }
    // A backedge into the entry would require the pre-permutation register
    // state. Entry zero remains valid, but every interior entry is a barrier.
    if instructions.iter().any(|i| match i {
        Branch { target } | BranchConditionalForward { target, .. } => (1..end).contains(target),
        _ => false,
    }) {
        return None;
    }
    if output
        .deferred_displacements
        .iter()
        .any(|d| d.instruction_index < end)
    {
        return None;
    }
    let prefix = &instructions[..multiply];
    let load = prefix.iter().position(|i| {
        matches!(
            i,
            LoadWord {
                d: 0,
                a: 3..=12,
                ..
            }
        )
    })?;
    let LoadWord { a: base, .. } = instructions[load] else {
        return None;
    };
    if base == multiplier {
        return None;
    }
    let address_high = prefix
        .iter()
        .position(|i| matches!(i, AddImmediateShifted { d, a: 0, .. } if *d == base))?;
    let magic_high = prefix
        .iter()
        .position(|i| matches!(i, AddImmediateShifted { d, a: 0, .. } if *d == multiplier))?;
    let magic_low = prefix.iter().position(
        |i| matches!(i, AddImmediate { d, a, .. } if *d == multiplier && *a == multiplier),
    )?;
    let lr_store = prefix.iter().position(|i| {
        matches!(
            i,
            StoreWord {
                s: 0,
                a: 1,
                offset: 4
            }
        )
    })?;
    let frame = prefix.iter().position(
        |i| matches!(i, StoreWordWithUpdate { s: 1, a: 1, offset } if *offset == -frame_size),
    )?;
    if lr_store > frame || address_high > load || magic_high > magic_low || lr_store > load {
        return None;
    }
    let saves: Vec<_> = prefix
        .iter()
        .enumerate()
        .filter_map(|(index, i)| {
            let valid = match i {
                StoreWord {
                    s: 14..=31,
                    a: 1,
                    offset,
                } => *offset >= 8 && i32::from(*offset) + 4 <= i32::from(frame_size),
                StoreMultipleWord {
                    s: first @ 14..=31,
                    a: 1,
                    offset,
                } => {
                    *offset >= 8
                        && i32::from(*offset) + 4 * i32::from(32 - *first) <= i32::from(frame_size)
                }
                _ => false,
            };
            (valid && index > frame).then_some(index)
        })
        .collect();
    if saves.iter().any(|&save| save >= load) {
        return None;
    }
    let relocations: Vec<_> = output
        .relocations
        .iter()
        .filter(|r| r.instruction_index < end)
        .collect();
    let anchor_packet = if relocations.is_empty() {
        None
    } else {
        let symbol = anchor?;
        if relocations.len() != 2 {
            return None;
        }
        let high = relocations
            .iter()
            .find(|r| {
                r.kind == RelocationKind::Addr16Ha
                    && matches!(&r.target, RelocationTarget::External(name) if name == symbol)
            })?
            .instruction_index;
        let low = relocations
            .iter()
            .find(|r| {
                r.kind == RelocationKind::Addr16Lo
                    && matches!(&r.target, RelocationTarget::External(name) if name == symbol)
            })?
            .instruction_index;
        let AddImmediateShifted {
            d: temporary @ 3..=12,
            a: 0,
            immediate: 0,
        } = instructions[high]
        else {
            return None;
        };
        let AddImmediate {
            d: saved @ 14..=31,
            a,
            immediate: 0,
        } = instructions[low]
        else {
            return None;
        };
        if saves.iter().any(|&save| save >= high)
            || a != temporary
            || high + 1 != low
            || low >= address_high
            || temporary == base
            || temporary == multiplier
            || !matches!(instructions[low], AddImmediate { d, a, immediate: 0 } if d == saved && a == temporary)
            || !saves.iter().any(|&i| match instructions[i] {
                StoreWord { s, .. } => s == saved,
                StoreMultipleWord { s, .. } => s <= saved,
                _ => false,
            })
        {
            return None;
        }
        let live = mwcc_vreg::analyze(instructions);
        if live_after(&live, multiplier, multiply) {
            return None;
        }
        if live_after(&live, temporary, low) {
            // Generic physical liveness conservatively treats every defined
            // argument lane as an input to the next call. A known scalar
            // prototype can prove this staging lane unused. Replace only those
            // calls with a definition of this lane in the liveness proof; the
            // emitted calls and all other paths remain untouched.
            let mut proof = instructions.clone();
            for instruction in &mut proof {
                if matches!(instruction, BranchAndLink { target } if call_ignores(target, temporary))
                {
                    *instruction = Instruction::load_immediate(temporary, 0);
                }
            }
            if live_after(&mwcc_vreg::analyze(&proof), temporary, low) {
                return None;
            }
        }
        Some((high, low, multiplier))
    };
    let mut order = vec![0, magic_high, lr_store, address_high, magic_low, frame];
    order.extend(saves);
    order.extend([load, multiply]);
    if let Some((high, low, _)) = anchor_packet {
        order.extend([high, low]);
    }
    order.push(multiply + 1);
    let mut covered = order.clone();
    covered.sort_unstable();
    if covered != (0..end).collect::<Vec<_>>() {
        return None;
    }
    Some(EntryPlan {
        order,
        anchor: anchor_packet,
    })
}

fn live_after(live: &Liveness, register: u8, index: usize) -> bool {
    live.pinned.iter().any(|range| {
        range.class == Class::General
            && range.register == register
            && range
                .live_slots
                .as_ref()
                .is_some_and(|slots| slots.binary_search(&(2 * index + 1)).is_ok())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::Relocation;
    use Instruction::*;

    fn entry_plan(output: &MachineFunction, frame: i16, anchor: Option<&str>) -> Option<EntryPlan> {
        entry_plan_with_calls(output, frame, anchor, |_, _| false)
    }

    #[test]
    fn a_known_call_prototype_can_end_an_unused_argument_lane() {
        let mut output = anchored();
        output.instructions[13] = BranchAndLink {
            target: "consume".into(),
        };
        output.instructions[12] = Instruction::load_immediate(3, 1);
        assert!(entry_plan(&output, 48, Some("section")).is_none());
        assert!(
            entry_plan_with_calls(&output, 48, Some("section"), |name, register| name
                == "consume"
                && register >= 4)
            .is_some()
        );
        output.instructions[12] = StoreWord {
            s: 5,
            a: 13,
            offset: 0,
        };
        assert!(entry_plan_with_calls(&output, 48, Some("section"), |_, _| true).is_none());
    }

    fn anchored() -> MachineFunction {
        let mut output = MachineFunction::new("entry");
        output.instructions = vec![
            MoveFromLinkRegister { d: 0 },
            StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            },
            StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 24,
            },
            AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: -32768,
            },
            AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0x51ec,
            },
            LoadWord {
                d: 0,
                a: 4,
                offset: 248,
            },
            AddImmediate {
                d: 3,
                a: 3,
                immediate: -31457,
            },
            MultiplyHighWordUnsigned { d: 0, a: 3, b: 0 },
            ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 7,
            },
            StoreWord {
                s: 0,
                a: 13,
                offset: 0,
            },
            BranchToLinkRegister,
        ];
        for (instruction_index, kind) in
            [(4, RelocationKind::Addr16Ha), (5, RelocationKind::Addr16Lo)]
        {
            output.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External("section".into()),
            });
        }
        output
    }

    #[test]
    fn interleaves_linkage_and_reuses_only_the_dead_multiplier() {
        let mut output = anchored();
        let plan = entry_plan(&output, 48, Some("section")).unwrap();
        assert_eq!(plan.order, [0, 7, 1, 6, 9, 2, 3, 8, 10, 4, 5, 11]);
        plan.apply(&mut output);
        assert!(matches!(
            output.instructions[9],
            AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0
            }
        ));
        assert!(matches!(
            output.instructions[10],
            AddImmediate {
                d: 31,
                a: 3,
                immediate: 0
            }
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|r| r.instruction_index)
                .collect::<Vec<_>>(),
            [9, 10]
        );
        assert!(entry_plan(&output, 48, Some("section")).is_none());
    }

    #[test]
    fn accepts_a_partially_scheduled_call_prefix_without_saved_homes() {
        let mut output = anchored();
        output.instructions.drain(3..6);
        output.relocations.clear();
        output.instructions[2] = StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -8,
        };
        crate::permute_machine_function_region(&mut output, 0, &[0, 3, 1, 4, 2, 5, 6, 7, 8]);
        let plan = entry_plan(&output, 8, None).unwrap();
        plan.apply(&mut output);
        assert!(matches!(
            output.instructions[1],
            AddImmediateShifted { d: 3, .. }
        ));
        assert!(matches!(
            output.instructions[3],
            AddImmediateShifted { d: 4, .. }
        ));
        assert!(matches!(
            output.instructions[4],
            AddImmediate { d: 3, a: 3, .. }
        ));
        assert!(matches!(output.instructions[5], StoreWordWithUpdate { .. }));
    }

    #[test]
    fn rejects_interior_entries_effects_and_unowned_relocations() {
        for target in 1..12 {
            let mut output = anchored();
            output.instructions.push(Branch { target });
            assert!(entry_plan(&output, 48, Some("section")).is_none());
        }
        for instruction in [
            StoreWord {
                s: 3,
                a: 4,
                offset: 0,
            },
            LoadWord {
                d: 3,
                a: 4,
                offset: 0,
            },
            BranchAndLink {
                target: "barrier".into(),
            },
            VerbatimWord(0),
        ] {
            let mut output = anchored();
            output.instructions[3] = instruction;
            assert!(entry_plan(&output, 48, Some("section")).is_none());
        }
        let mut output = anchored();
        output.relocations[0].instruction_index = 7;
        assert!(entry_plan(&output, 48, Some("section")).is_none());
        assert!(entry_plan(&anchored(), 48, Some("other")).is_none());
    }

    #[test]
    fn rejects_live_temporaries_and_missing_saved_homes() {
        for register in [3, 5] {
            let mut output = anchored();
            output.instructions[12] = StoreWord {
                s: register,
                a: 13,
                offset: 0,
            };
            assert!(entry_plan(&output, 48, Some("section")).is_none());
        }
        let mut late_save = anchored();
        crate::permute_machine_function_region(
            &mut late_save,
            0,
            &[0, 1, 2, 4, 5, 3, 6, 7, 8, 9, 10, 11],
        );
        assert!(entry_plan(&late_save, 48, Some("section")).is_none());
        let mut output = anchored();
        output.instructions[3] = StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        };
        assert!(entry_plan(&output, 48, Some("section")).is_none());
    }
}
