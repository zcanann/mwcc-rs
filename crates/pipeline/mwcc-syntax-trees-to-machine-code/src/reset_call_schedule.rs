//! Keep a stable callback pointer in r3 while issuing a member reset loop.
//!
//! A loop with one terminal callback can stage its saved pointer while the reset
//! stores issue. The old temporary lanes move up only after a forward-flow proof
//! establishes that they have no incoming values or escaping call arguments.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction as I, MachineFunction};
use mwcc_syntax_trees::Type;
use mwcc_versions::{FrameConvention, Optimization, OptimizationGoal};
use mwcc_vreg::{for_each_register, register_operands, Class, RegisterRole};

impl Generator {
    pub(crate) fn schedule_reset_loop_callback(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization < Optimization::O3
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.preceded_by_asm
        {
            return;
        }
        let loops: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(latch, i)| match i {
                I::BranchConditionalForward {
                    options, target, ..
                } if *options != 16 && *target < latch => Some((*target, latch)),
                _ => None,
            })
            .collect();
        for (start, latch) in loops {
            let Some(call) = self.output.instructions[start..latch]
                .iter()
                .rposition(|i| matches!(i, I::BranchAndLink { .. }))
                .map(|at| start + at)
            else {
                continue;
            };
            let I::BranchAndLink { target } = &self.output.instructions[call] else {
                unreachable!();
            };
            let Some(parameters) = self.call_parameter_types.get(target) else {
                continue;
            };
            if self.call_return_types.get(target) != Some(&Type::Void)
                || self.variadic_callees.contains(target)
                || parameters.is_empty()
                || parameters.len() > 8
                || !matches!(parameters[0], Type::Pointer(_) | Type::StructPointer { .. })
                || parameters.iter().any(|ty| {
                    ty.width() > 32 || matches!(ty, Type::Float | Type::Void | Type::Struct { .. })
                })
            {
                continue;
            }
            if let Some(plan) = plan(
                &self.output,
                start,
                call,
                latch,
                parameters.len(),
                self.behavior.early_reset_comparison,
            ) {
                plan.apply(&mut self.output, start);
            }
        }
    }
}

struct Plan {
    arguments: usize,
    pointer: u32,
    order: Vec<usize>,
}

impl Plan {
    fn apply(self, output: &mut MachineFunction, start: usize) {
        for instruction in &mut output.instructions[start..self.arguments] {
            for_each_register(instruction, |_, class, register| {
                if class == Class::General && (3..=11).contains(register) {
                    *register += 1;
                }
            });
        }
        output.instructions[self.arguments] = I::move_register(3, self.pointer);
        crate::permute_machine_function_region(output, start, &self.order);
    }
}

fn saved_copy(i: &I, destination: u32) -> Option<u32> {
    match *i {
        I::AddImmediate {
            d,
            a: source @ 14..=31,
            immediate: 0,
        } if d == destination => Some(source),
        I::Or {
            a,
            s: source @ 14..=31,
            b,
        } if a == destination && source == b => Some(source),
        _ => None,
    }
}

fn plan(
    f: &MachineFunction,
    start: usize,
    call: usize,
    latch: usize,
    parameters: usize,
    early_compare: bool,
) -> Option<Plan> {
    let code = &f.instructions;
    let arguments = call.checked_sub(parameters)?;
    if arguments < start + 8
        || f.is_asm
        || !f.entry_points.is_empty()
        || !f.jump_tables.is_empty()
        || code.iter().any(|i| matches!(i, I::VerbatimWord(_)))
        || f.relocations
            .iter()
            .any(|r| (start..call).contains(&r.instruction_index))
        || f.deferred_displacements
            .iter()
            .any(|d| (start..call).contains(&d.instruction_index))
    {
        return None;
    }
    let mut inputs = Vec::new();
    for n in 0..parameters {
        inputs.push(saved_copy(&code[arguments + n], (3 + n as u8).into())?);
    }
    let pointer = inputs[0];
    let [I::StoreWord {
        s: index @ 14..=31,
        a: owner @ 14..=31,
        ..
    }, I::AddImmediate {
        d: 4,
        a: address_base,
        ..
    }, I::StoreWord {
        s: 4,
        a: published_base,
        offset: published_offset,
    }, I::AddImmediate { d: 3, a: 0, .. }, I::AddImmediate { d: 0, a: 0, .. }, I::StoreWord {
        s: 14..=31,
        a: other_base,
        ..
    }] = code.get(start..start + 6)?
    else {
        return None;
    };
    if owner != address_base || owner != published_base || owner != other_base {
        return None;
    }
    let mut compare = start + 6;
    let mut shared_address = false;
    let mut half_reset = false;
    let mut tag = false;
    while compare < arguments {
        match code[compare] {
            I::StoreWord { s: 4, a, offset } if a == *owner && offset == *published_offset => {
                shared_address = true
            }
            I::StoreWord { s: 0, a, .. } if a == *owner => tag = true,
            I::StoreWord { s: 3, a, .. } if a == *owner => {}
            I::StoreHalfword { s: 3, a, .. } if a == *owner => half_reset = true,
            _ => break,
        }
        compare += 1;
    }
    if !shared_address
        || !half_reset
        || !tag
        || !matches!(code.get(compare), Some(I::CompareLogicalWordImmediate { a, .. } | I::CompareWordImmediate { a, .. }) if a == index)
        || !matches!(
            code.get(compare + 1),
            Some(I::BranchConditionalForward { .. })
        )
    {
        return None;
    }
    // Every entry executes the setup. Interior branches stay in the reset's
    // forward CFG and cannot enter the argument copy that will move earlier.
    if code.iter().enumerate().any(|(at, i)| match i {
        I::Branch { target } | I::BranchConditionalForward { target, .. } => {
            if (start..call).contains(&at) {
                *target <= at || *target <= compare || *target >= arguments
            } else {
                start < *target && *target < call
            }
        }
        _ => false,
    }) || code[start..arguments].iter().any(|i| {
        !ordinary(i)
            || register_operands(i).iter().any(|r| {
                r.class == Class::General && r.role == RegisterRole::Define && r.register == pointer.into()
            })
    }) || code[call + 1..latch].iter().any(|i| {
        !matches!(
            i,
            I::AddImmediate {
                d: 14..=31,
                a: 14..=31,
                ..
            } | I::CompareLogicalWordImmediate { a: 14..=31, .. }
                | I::CompareWordImmediate { a: 14..=31, .. }
        )
    }) || !local_scratch(code, start, arguments)
    {
        return None;
    }

    let cmp = compare - start;
    let arg = arguments - start;
    let mut order = vec![0, 1, 3];
    if early_compare {
        order.push(cmp);
    }
    order.extend([2, 4]);
    if !early_compare {
        order.push(cmp);
    }
    order.extend([5, arg]);
    order.extend((6..call - start).filter(|at| *at != cmp && *at != arg));
    // The terminal tag can issue immediately after the last r0 dependency in
    // the straight-line join. Incoming branch targets must remain before it.
    if let Some(literal) = arguments.checked_sub(2).filter(|&at| at > compare) {
        if matches!(code[literal], I::AddImmediate { d: 0, a: 0, .. })
            && matches!(code[literal + 1], I::StoreWord { s: 0, a, .. } if a == *owner)
        {
            let floor = code[compare + 1..literal]
                .iter()
                .enumerate()
                .filter_map(|(n, i)| match i {
                    I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                        Some((compare + 2 + n).max(target + 1))
                    }
                    _ => None,
                })
                .max()
                .unwrap_or(compare + 2);
            let after = (floor..literal)
                .rfind(|&at| {
                    register_operands(&code[at])
                        .iter()
                        .any(|r| r.class == Class::General && r.register == 0)
                })
                .map_or(floor, |at| at + 1);
            if after < literal {
                order.retain(|&at| at != literal - start);
                let insertion = order.iter().position(|&at| at == after - start)?;
                order.insert(insertion, literal - start);
            }
        }
    }
    Some(Plan {
        arguments,
        pointer: pointer.into(),
        order,
    })
}

fn ordinary(i: &I) -> bool {
    matches!(
        i,
        I::AddImmediate { .. }
            | I::AddImmediateShifted { .. }
            | I::StoreWord { .. }
            | I::StoreHalfword { .. }
            | I::StoreByte { .. }
            | I::ShiftRightLogicalImmediate { .. }
            | I::ShiftLeftImmediate { .. }
            | I::Or { .. }
            | I::CompareLogicalWordImmediate { .. }
            | I::CompareWordImmediate { .. }
            | I::Branch { .. }
            | I::BranchConditionalForward { .. }
    )
}

/// Forward dataflow intersects definitions at each merge. A use of any old
/// scratch value must have a definition on every incoming path. r12 cannot be
/// shifted to another volatile lane and therefore prevents the rewrite.
fn local_scratch(code: &[I], start: usize, end: usize) -> bool {
    let mut states = vec![None; end - start + 1];
    states[0] = Some(0u16);
    for at in start..end {
        let Some(mut defined) = states[at - start] else {
            continue;
        };
        let operands = register_operands(&code[at]);
        for r in &operands {
            if r.class != Class::General {
                continue;
            }
            if r.register == 12
                || ((3..=11).contains(&r.register)
                    && r.role == RegisterRole::Use
                    && defined & (1 << r.register) == 0)
            {
                return false;
            }
        }
        for r in &operands {
            if r.class == Class::General
                && r.role == RegisterRole::Define
                && (3..=11).contains(&r.register)
            {
                defined |= 1 << r.register;
            }
        }
        let mut targets = vec![at + 1];
        match code[at] {
            I::Branch { target } => targets = vec![target],
            I::BranchConditionalForward { target, .. } => targets.push(target),
            _ => {}
        }
        for target in targets {
            if target <= at || target > end {
                return false;
            }
            let entry = &mut states[target - start];
            *entry = Some(entry.map_or(defined, |prior| prior & defined));
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    fn fixture() -> MachineFunction {
        let mut f = MachineFunction::new("reset_loop");
        f.instructions = vec![
            I::StoreWord {
                s: 26,
                a: 27,
                offset: 0,
            },
            I::AddImmediate {
                d: 4,
                a: 27,
                immediate: 8,
            },
            I::StoreWord {
                s: 4,
                a: 27,
                offset: 4,
            },
            I::load_immediate(3, 0),
            I::load_immediate(0, 164),
            I::StoreWord {
                s: 29,
                a: 27,
                offset: 24,
            },
            I::StoreHalfword {
                s: 3,
                a: 27,
                offset: 44,
            },
            I::StoreWord {
                s: 0,
                a: 27,
                offset: 28,
            },
            I::StoreWord {
                s: 3,
                a: 27,
                offset: 32,
            },
            I::StoreWord {
                s: 4,
                a: 27,
                offset: 4,
            },
            I::CompareLogicalWordImmediate {
                a: 26,
                immediate: 63,
            },
            I::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 15,
            },
            I::StoreHalfword {
                s: 3,
                a: 27,
                offset: 60,
            },
            I::StoreHalfword {
                s: 3,
                a: 27,
                offset: 62,
            },
            I::Branch { target: 18 },
            I::AddImmediate {
                d: 0,
                a: 30,
                immediate: 4,
            },
            I::ShiftRightLogicalImmediate {
                a: 3,
                s: 0,
                shift: 16,
            },
            I::StoreHalfword {
                s: 3,
                a: 27,
                offset: 60,
            },
            I::ShiftRightLogicalImmediate {
                a: 0,
                s: 30,
                shift: 16,
            },
            I::StoreHalfword {
                s: 0,
                a: 27,
                offset: 64,
            },
            I::ShiftRightLogicalImmediate {
                a: 3,
                s: 28,
                shift: 16,
            },
            I::StoreHalfword {
                s: 3,
                a: 27,
                offset: 68,
            },
            I::StoreHalfword {
                s: 30,
                a: 27,
                offset: 66,
            },
            I::load_immediate(0, 1),
            I::StoreWord {
                s: 0,
                a: 27,
                offset: 40,
            },
            I::AddImmediate {
                d: 3,
                a: 27,
                immediate: 0,
            },
            I::BranchAndLink {
                target: "observe".into(),
            },
            I::AddImmediate {
                d: 26,
                a: 26,
                immediate: 1,
            },
            I::CompareLogicalWordImmediate {
                a: 26,
                immediate: 64,
            },
            I::AddImmediate {
                d: 27,
                a: 27,
                immediate: 72,
            },
            I::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 0,
            },
            I::BranchToLinkRegister,
        ];
        f.relocations.push(Relocation {
            instruction_index: 26,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("observe".into()),
        });
        f
    }

    #[test]
    fn stages_the_callback_with_profile_comparison_order_and_unchanged_memory_order() {
        for early in [false, true] {
            let mut f = fixture();
            let old = f.instructions.clone();
            let p = plan(&f, 0, 26, 30, 1, early).unwrap();
            let order = p.order.clone();
            assert_eq!(
                &order[..8],
                if early {
                    &[0, 1, 3, 10, 2, 4, 5, 25]
                } else {
                    &[0, 1, 3, 2, 4, 10, 5, 25]
                }
            );
            assert_eq!(
                order.iter().position(|i| *i == 23),
                order.iter().position(|i| *i == 19).map(|i| i + 1)
            );
            p.apply(&mut f, 0);
            assert_eq!(
                f.instructions[1],
                I::AddImmediate {
                    d: 5,
                    a: 27,
                    immediate: 8
                }
            );
            assert_eq!(f.instructions[2], I::load_immediate(4, 0));
            assert_eq!(f.instructions[7], I::move_register(3, 27));
            let stores = |code: &[I]| {
                code.iter()
                    .filter(|i| matches!(i, I::StoreWord { .. } | I::StoreHalfword { .. }))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            let mut expected = stores(&old);
            for i in &mut expected {
                for_each_register(i, |_, class, r| {
                    if class == Class::General && (3..=11).contains(r) {
                        *r += 1;
                    }
                });
            }
            assert_eq!(stores(&f.instructions), expected);
            for (old_at, i) in old[..26].iter().enumerate() {
                if let I::Branch { target } | I::BranchConditionalForward { target, .. } = i {
                    let new_at = order.iter().position(|i| *i == old_at).unwrap();
                    let expected_target = order.iter().position(|i| i == target).unwrap();
                    assert!(
                        matches!(f.instructions[new_at], I::Branch { target } | I::BranchConditionalForward { target, .. } if target == expected_target)
                    );
                }
            }
            assert_eq!(f.instructions[26..], old[26..]);
            assert_eq!(f.relocations[0].instruction_index, 26);
            assert!(plan(&f, 0, 26, 30, 1, early).is_none());
        }
    }

    #[test]
    fn later_saved_arguments_keep_their_abi_lanes() {
        let mut f = fixture();
        f.instructions.insert(26, I::move_register(4, 26));
        f.relocations[0].instruction_index += 1;
        let p = plan(&f, 0, 27, 31, 2, false).unwrap();
        p.apply(&mut f, 0);
        assert_eq!(f.instructions[26], I::move_register(4, 26));
        assert_eq!(f.instructions[7], I::move_register(3, 27));
    }

    #[test]
    fn pointer_identity_is_not_tied_to_the_reset_owner() {
        let mut f = fixture();
        f.instructions[25] = I::move_register(3, 30);
        let p = plan(&f, 0, 26, 30, 1, false).unwrap();
        p.apply(&mut f, 0);
        assert_eq!(f.instructions[7], I::move_register(3, 30));
    }

    #[test]
    fn calls_live_inputs_and_rebound_pointers_prevent_staging() {
        for variant in 0..4 {
            let mut f = fixture();
            f.instructions[20] = match variant {
                0 => I::BranchAndLink {
                    target: "barrier".into(),
                },
                1 => I::StoreWord {
                    s: 5,
                    a: 27,
                    offset: 8,
                },
                2 => I::AddImmediate {
                    d: 27,
                    a: 27,
                    immediate: 72,
                },
                _ => I::load_immediate(12, 7),
            };
            assert!(plan(&f, 0, 26, 30, 1, false).is_none());
        }
    }

    #[test]
    fn side_entries_argument_bypasses_and_relocated_reset_stores_are_rejected() {
        for variant in 0..4 {
            let mut f = fixture();
            match variant {
                0 => f.instructions.push(I::Branch { target: 3 }),
                1 => f.instructions[14] = I::Branch { target: 25 },
                2 => f.relocations.push(Relocation {
                    instruction_index: 7,
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External("global".into()),
                }),
                _ => {
                    f.instructions[9] = I::StoreWord {
                        s: 4,
                        a: 27,
                        offset: 12,
                    }
                }
            }
            assert!(plan(&f, 0, 26, 30, 1, false).is_none());
        }
    }

    #[test]
    fn a_join_at_the_terminal_literal_keeps_that_definition_at_the_join() {
        let mut f = fixture();
        f.instructions[14] = I::Branch { target: 23 };
        let p = plan(&f, 0, 26, 30, 1, false).unwrap();
        let literal = p.order.iter().position(|i| *i == 23).unwrap();
        assert_eq!(p.order[literal + 1], 24);
        p.apply(&mut f, 0);
        assert!(f
            .instructions
            .iter()
            .any(|i| matches!(i, I::Branch { target } if *target == literal)));
    }

    #[test]
    fn scratch_definitions_must_dominate_reads_on_both_paths() {
        let missing = [
            I::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 2,
            },
            I::load_immediate(3, 0),
            I::StoreWord {
                s: 3,
                a: 27,
                offset: 0,
            },
        ];
        assert!(!local_scratch(&missing, 0, missing.len()));
        let complete = [
            I::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 3,
            },
            I::load_immediate(3, 0),
            I::Branch { target: 4 },
            I::load_immediate(3, 7),
            I::StoreWord {
                s: 3,
                a: 27,
                offset: 0,
            },
        ];
        assert!(local_scratch(&complete, 0, complete.len()));
    }
}
