//! Schedule successive CTR fills after complete section addresses have expanded.
//!
//! The early scheduler reuses r3 for a disposable fill pointer. Its narrow
//! address follows the trip count; a wide address straddles the count and fill
//! constants. Layout owns the address halves, so this belongs after expansion.

use mwcc_machine_code::{FixedFillAddressSchedule, Instruction as I, MachineFunction};
use mwcc_vreg::{register_operands, Class, RegisterRole};

pub(super) fn schedule(function: &mut MachineFunction, address_owners: &[usize]) {
    let Some(policy) = function.following_fixed_fill_schedule else {
        return;
    };
    if function.is_asm
        || !function.entry_points.is_empty()
        || !function.jump_tables.is_empty()
        || function
            .instructions
            .iter()
            .any(|i| matches!(i, I::VerbatimWord(_)))
    {
        return;
    }
    for &fixup in address_owners {
        let address = function.deferred_displacements[fixup].instruction_index;
        let Some(plan) = plan(function, address, policy) else {
            continue;
        };
        for instruction in &mut function.instructions[plan.start..plan.end] {
            match instruction {
                I::AddImmediate { d, a, .. } | I::AddImmediateShifted { d, a, .. } => {
                    if *d == plan.pointer {
                        *d = 3;
                    }
                    if *a == plan.pointer {
                        *a = 3;
                    }
                }
                I::StoreWord { a, .. } => *a = 3,
                _ => {}
            }
        }
        let old = function.instructions[plan.start..plan.start + plan.order.len()].to_vec();
        for (new, &old_index) in plan.order.iter().enumerate() {
            function.instructions[plan.start + new] = old[old_index].clone();
        }
        // The packet has one displacement owner, no relocations or side entries,
        // and its store body/backedge do not move. Only the low owner moves.
        function.deferred_displacements[fixup].instruction_index = plan.start
            + plan
                .order
                .iter()
                .position(|&i| i == address - plan.start)
                .unwrap();
    }
}

struct Plan {
    start: usize,
    end: usize,
    pointer: u32,
    order: &'static [usize],
}

fn plan(f: &MachineFunction, address: usize, policy: FixedFillAddressSchedule) -> Option<Plan> {
    let code = &f.instructions;
    let I::AddImmediate {
        d: pointer @ 3..=12,
        a: base,
        ..
    } = *code.get(address)?
    else {
        return None;
    };
    let wide = base == pointer
        && matches!(address.checked_sub(1).and_then(|i| code.get(i)), Some(I::AddImmediateShifted { d, a: 14..=31, .. }) if *d == pointer);
    if !wide && !(14..=31).contains(&base) {
        return None;
    }
    let start = address - usize::from(wide);
    // The entry fill has a different publication schedule. This pass owns only
    // a fill immediately following another completed CTR packet.
    if !matches!(start.checked_sub(1).and_then(|i| code.get(i)), Some(I::BranchConditionalForward { options: 16, condition_bit: 0, target }) if *target < start - 1)
    {
        return None;
    }
    let [I::AddImmediate {
        d: 0,
        a: 0,
        immediate: count,
    }, I::MoveToCountRegister { s: 0 }, I::AddImmediate { d: 0, a: 0, .. }] =
        code.get(address + 1..address + 4)?
    else {
        return None;
    };
    if *count < 2 {
        return None;
    }
    let body = address + 4;
    let stores = code[body..].iter().take(10).enumerate().take_while(|(n, i)| matches!(i, I::StoreWord { s: 0, a, offset } if *a == pointer && *offset == 4 * *n as i16)).count();
    if stores == 0
        || !matches!(code.get(body + stores), Some(I::AddImmediate { d, a, immediate }) if *d == pointer && *a == pointer && *immediate == 4 * stores as i16)
        || !matches!(code.get(body + stores + 1), Some(I::BranchConditionalForward { options: 16, condition_bit: 0, target }) if *target == body)
    {
        return None;
    }
    let end = body + stores + 2;
    if f.relocations
        .iter()
        .any(|r| (start..end).contains(&r.instruction_index))
        || f.deferred_displacements
            .iter()
            .filter(|d| (start..end).contains(&d.instruction_index))
            .count()
            != 1
        || code.iter().enumerate().any(|(at, i)| match i {
            I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                (start..end).contains(target) && !(at == end - 1 && *target == body)
            }
            _ => false,
        })
        || (pointer != 3
            && (!dead_until_definition(code, end, pointer) || !dead_until_definition(code, end, 3)))
    {
        return None;
    }
    let order: &[usize] = if wide {
        &[2, 0, 3, 4, 1]
    } else if policy == FixedFillAddressSchedule::BeforeCountRegister {
        &[1, 0, 2, 3]
    } else {
        &[1, 2, 0, 3]
    };
    Some(Plan {
        start,
        end,
        pointer,
        order,
    })
}

/// Prove that every reachable path overwrites the old value before reading it.
/// Calls and exits are barriers: implicit argument/result uses remain observable.
/// Revisiting a loop is safe once its instructions have been checked for uses.
fn dead_until_definition(code: &[I], start: usize, register: u32) -> bool {
    let mut pending = vec![start];
    let mut visited = vec![false; code.len()];
    while let Some(at) = pending.pop() {
        let Some(instruction) = code.get(at) else {
            return false;
        };
        if visited[at] {
            continue;
        }
        visited[at] = true;
        let operands = register_operands(instruction);
        if operands.iter().any(|o| {
            o.class == Class::General && o.register == register && o.role == RegisterRole::Use
        }) {
            return false;
        }
        if operands.iter().any(|o| {
            o.class == Class::General && o.register == register && o.role == RegisterRole::Define
        }) {
            continue;
        }
        match instruction {
            I::BranchAndLink { .. }
            | I::BranchToCountRegisterAndLink
            | I::BranchToLinkRegisterAndLink
            | I::BranchToLinkRegister
            | I::BranchToCountRegister
            | I::BranchExternal { .. }
            | I::BranchConditionalToLinkRegister { .. }
            | I::ReturnFromInterrupt
            | I::SystemCall
            | I::VerbatimWord(_) => return false,
            I::Branch { target } => pending.push(*target),
            I::BranchConditionalForward { target, .. } => {
                pending.push(*target);
                pending.push(at + 1);
            }
            _ => pending.push(at + 1),
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{
        DeferredDisplacement, DeferredDisplacementTarget, Relocation, RelocationKind,
        RelocationTarget,
    };

    fn packet(wide: bool) -> (MachineFunction, usize, usize) {
        let mut f = MachineFunction::new("following_fill");
        f.following_fixed_fill_schedule = Some(FixedFillAddressSchedule::AfterCountRegister);
        f.instructions = vec![
            I::StoreWord {
                s: 3,
                a: 4,
                offset: 0,
            },
            I::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 0,
            },
        ];
        if wide {
            f.instructions.push(I::AddImmediateShifted {
                d: 4,
                a: 31,
                immediate: 1,
            });
        }
        let address = f.instructions.len();
        f.instructions.extend([
            I::AddImmediate {
                d: 4,
                a: if wide { 4 } else { 31 },
                immediate: 0,
            },
            I::AddImmediate {
                d: 0,
                a: 0,
                immediate: 128,
            },
            I::MoveToCountRegister { s: 0 },
            I::AddImmediate {
                d: 0,
                a: 0,
                immediate: 7,
            },
        ]);
        let body = f.instructions.len();
        for n in 0..8 {
            f.instructions.push(I::StoreWord {
                s: 0,
                a: 4,
                offset: 4 * n,
            });
        }
        f.instructions.extend([
            I::AddImmediate {
                d: 4,
                a: 4,
                immediate: 32,
            },
            I::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: body,
            },
            I::AddImmediate {
                d: 3,
                a: 0,
                immediate: 99,
            },
            I::AddImmediate {
                d: 4,
                a: 0,
                immediate: 55,
            },
            I::BranchToLinkRegister,
        ]);
        f.deferred_displacements.push(DeferredDisplacement {
            instruction_index: address,
            target: DeferredDisplacementTarget::Symbol("buffer".into()),
        });
        (f, address, body)
    }

    #[test]
    fn narrow_schedule_and_low_owner_follow_the_selected_policy() {
        for (policy, expected) in [
            (FixedFillAddressSchedule::AfterCountRegister, [1, 2, 0, 3]),
            (FixedFillAddressSchedule::BeforeCountRegister, [1, 0, 2, 3]),
        ] {
            let (mut f, address, body) = packet(false);
            f.following_fixed_fill_schedule = Some(policy);
            let old = f.instructions.clone();
            schedule(&mut f, &[0]);
            for (new, index) in expected.into_iter().enumerate() {
                let mut instruction = old[2 + index].clone();
                if let I::AddImmediate { d: d @ 4, .. } = &mut instruction {
                    *d = 3;
                }
                assert_eq!(f.instructions[2 + new], instruction);
            }
            assert_eq!(
                f.deferred_displacements[0].instruction_index,
                2 + expected.iter().position(|i| *i == address - 2).unwrap()
            );
            assert!(f.instructions[body..body + 8]
                .iter()
                .all(|i| matches!(i, I::StoreWord { s: 0, a: 3, .. })));
            assert_eq!(
                f.instructions[body + 8],
                I::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 32
                }
            );
            assert_eq!(f.instructions[body + 9..], old[body + 9..]);
            let once = f.instructions.clone();
            schedule(&mut f, &[0]);
            assert_eq!(f.instructions, once);
        }
    }

    #[test]
    fn wide_address_straddles_count_and_fill_without_moving_the_backedge() {
        for policy in [
            FixedFillAddressSchedule::AfterCountRegister,
            FixedFillAddressSchedule::BeforeCountRegister,
        ] {
            let (mut f, _, body) = packet(true);
            f.following_fixed_fill_schedule = Some(policy);
            schedule(&mut f, &[0]);
            assert_eq!(
                &f.instructions[2..body],
                &[
                    I::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 128
                    },
                    I::AddImmediateShifted {
                        d: 3,
                        a: 31,
                        immediate: 1
                    },
                    I::MoveToCountRegister { s: 0 },
                    I::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 7
                    },
                    I::AddImmediate {
                        d: 3,
                        a: 3,
                        immediate: 0
                    },
                ]
            );
            assert_eq!(f.deferred_displacements[0].instruction_index, body - 1);
            assert!(
                matches!(f.instructions[body + 9], I::BranchConditionalForward { target, .. } if target == body)
            );
        }
    }

    #[test]
    fn live_pointer_or_live_replacement_prevents_recoloring() {
        for register in [3, 4] {
            let (mut f, _, body) = packet(true);
            f.instructions[body + 10] = I::StoreWord {
                s: register,
                a: 31,
                offset: 0,
            };
            let old = f.instructions.clone();
            schedule(&mut f, &[0]);
            assert_eq!(f.instructions, old);
        }
    }

    #[test]
    fn side_entries_relocations_and_unowned_displacements_prevent_scheduling() {
        for variant in 0..5 {
            let (mut f, address, body) = packet(true);
            match variant {
                0 => f.instructions.push(I::Branch { target: address }),
                1 => f.instructions.push(I::Branch { target: body }),
                2 => f.relocations.push(Relocation {
                    instruction_index: address,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::External("other".into()),
                }),
                3 => f
                    .deferred_displacements
                    .push(f.deferred_displacements[0].clone()),
                _ => f.entry_points.push(("entry".into(), body)),
            }
            let old = f.instructions.clone();
            schedule(&mut f, &[0]);
            assert_eq!(f.instructions, old);
        }
    }

    #[test]
    fn incomplete_store_packet_and_unselected_policy_are_unchanged() {
        for variant in 0..4 {
            let (mut f, _, body) = packet(false);
            match variant {
                0 => f.following_fixed_fill_schedule = None,
                1 => {
                    f.instructions[body + 1] = I::StoreWord {
                        s: 3,
                        a: 4,
                        offset: 4,
                    }
                }
                2 => {
                    f.instructions[body + 8] = I::AddImmediate {
                        d: 4,
                        a: 4,
                        immediate: 36,
                    }
                }
                _ => f.is_asm = true,
            }
            let old = f.instructions.clone();
            schedule(&mut f, &[0]);
            assert_eq!(f.instructions, old);
        }
    }

    #[test]
    fn dead_value_proof_checks_both_branch_paths_and_read_modify_write() {
        let mut code = vec![
            I::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 2,
            },
            I::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0,
            },
            I::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            },
        ];
        assert!(!dead_until_definition(&code, 0, 3));
        code[2] = I::AddImmediate {
            d: 3,
            a: 0,
            immediate: 1,
        };
        assert!(dead_until_definition(&code, 0, 3));
        code[1] = I::Branch { target: 0 };
        assert!(dead_until_definition(&code, 0, 3));
    }

    #[test]
    fn calls_and_all_exits_block_the_dead_value_proof() {
        for barrier in [
            I::BranchAndLink {
                target: "observe".into(),
            },
            I::BranchExternal {
                target: "tail".into(),
            },
            I::BranchToCountRegisterAndLink,
            I::BranchToLinkRegisterAndLink,
            I::BranchToLinkRegister,
            I::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            },
            I::BranchToCountRegister,
            I::SystemCall,
            I::ReturnFromInterrupt,
            I::VerbatimWord(0),
        ] {
            assert!(!dead_until_definition(
                &[
                    barrier,
                    I::AddImmediate {
                        d: 3,
                        a: 0,
                        immediate: 0
                    }
                ],
                0,
                3
            ));
        }
        assert!(!dead_until_definition(&[], 0, 3));
    }
}
