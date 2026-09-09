//! Issue wide cursor high halves before independent narrow addresses and index.
//!
//! Source strength reduction nominates the array group. Object layout owns its
//! complete offsets. Independent setup instructions may move, and completed
//! pages lose their redundant low add. Register homes and loop operations stay
//! fixed; surviving instruction owners and branch boundaries follow any shrink.

use mwcc_machine_code::{DeferredDisplacementTarget as Target, Instruction as I, MachineFunction};
use std::collections::{HashMap, HashSet};

pub(super) fn schedule(function: &mut MachineFunction, resolved: &[(usize, u32)]) {
    let groups = std::mem::take(&mut function.anchored_cursor_address_groups);
    if groups.is_empty()
        || function.is_asm
        || !function.jump_tables.is_empty()
        || !function.entry_points.is_empty()
        || function
            .instructions
            .iter()
            .any(|i| matches!(i, I::VerbatimWord(_)))
    {
        return;
    }
    let owned: HashMap<_, _> = resolved.iter().copied().collect();
    let mut plans = Vec::new();
    for group in groups {
        if group.len() < 3 || group.iter().collect::<HashSet<_>>().len() != group.len() {
            continue;
        }
        let mut start = 0;
        while start < function.instructions.len() {
            let Some(plan) = plan(function, start, &group, &owned) else {
                start += 1;
                continue;
            };
            let end = start + plan.old_length;
            if !plans.iter().any(|(other_start, other): &(usize, Plan)| {
                start < other_start + other.old_length && *other_start < end
            }) {
                plans.push((start, plan));
            }
            start = end;
        }
    }
    // Validate against the original ownership graph, then rewrite from the end.
    // Removing a completed page's low owner cannot invalidate another plan.
    plans.sort_by_key(|(start, _)| std::cmp::Reverse(*start));
    for (start, plan) in plans {
        apply(function, start, plan);
    }
}

struct Plan {
    old_length: usize,
    order: Vec<usize>,
}

fn apply(function: &mut MachineFunction, start: usize, plan: Plan) {
    let end = start + plan.old_length;
    let removed = plan.old_length - plan.order.len();
    let old = function.instructions[start..end].to_vec();
    let replacement = plan
        .order
        .iter()
        .map(|&at| old[at].clone())
        .collect::<Vec<_>>();
    function.instructions.splice(start..end, replacement);
    let boundary = |at: usize| if at >= end { at - removed } else { at };
    function.deferred_displacements.retain_mut(|d| {
        if (start..end).contains(&d.instruction_index) {
            let Some(new) = plan
                .order
                .iter()
                .position(|&at| at == d.instruction_index - start)
            else {
                return false;
            };
            d.instruction_index = start + new;
        } else {
            d.instruction_index = boundary(d.instruction_index);
        }
        true
    });
    // No interior relocation or entry survived validation. Every incoming edge
    // enters the whole packet or its continuation, not a moved definition.
    for relocation in &mut function.relocations {
        relocation.instruction_index = boundary(relocation.instruction_index);
    }
    for instruction in &mut function.instructions {
        match instruction {
            I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                *target = boundary(*target)
            }
            _ => {}
        }
    }
}

fn plan(
    f: &MachineFunction,
    start: usize,
    symbols: &[String],
    owned: &HashMap<usize, u32>,
) -> Option<Plan> {
    let code = &f.instructions;
    let I::AddImmediate {
        d: index @ 14..=31,
        a: 0,
        immediate: 0,
    } = *code.get(start)?
    else {
        return None;
    };
    let mut at = start + 1;
    let mut registers = HashSet::from([index]);
    let mut anchor = None;
    let mut high = Vec::new();
    let mut narrow = Vec::new();
    let mut low = Vec::new();
    let mut zero = None;
    let mut complete_pages = Vec::new();
    for name in symbols {
        let wide = matches!(code.get(at), Some(I::AddImmediateShifted { .. }));
        let address = at + usize::from(wide);
        let I::AddImmediate {
            d: destination @ 14..=31,
            a,
            immediate: 0,
        } = *code.get(address)?
        else {
            return None;
        };
        let base = if wide {
            let I::AddImmediateShifted {
                d,
                a: base @ 14..=31,
                ..
            } = code[at]
            else {
                return None;
            };
            if d != destination || a != destination {
                return None;
            }
            base
        } else {
            a
        };
        if !(14..=31).contains(&base)
            || !registers.insert(destination)
            || anchor.is_some_and(|r| r != base)
        {
            return None;
        }
        anchor = Some(base);
        let fixups: Vec<_> = f
            .deferred_displacements
            .iter()
            .enumerate()
            .filter(|(_, d)| d.instruction_index == address)
            .collect();
        let [(fixup, displacement)] = fixups.as_slice() else {
            return None;
        };
        if !matches!(&displacement.target, Target::Symbol(symbol) if symbol == name) {
            return None;
        }
        let offset = *owned.get(fixup)?;
        if wide && offset as u16 == 0 {
            // The high instruction already computes the complete address. The
            // low fixup may disappear only if symbol discovery owns its name.
            if !f.symbol_order.contains(name) {
                return None;
            }
            narrow.push(at - start);
            complete_pages.push(at - start);
        } else if wide {
            high.push(at - start);
            low.push(address - start);
        } else {
            if offset == 0 {
                zero = Some(narrow.len());
            }
            narrow.push(address - start);
        }
        at = address + 1;
    }
    // Narrow-only packets do not need this layout-owned wide-address schedule.
    if high.is_empty()
        || registers.contains(&anchor?)
        || f.relocations
            .iter()
            .any(|r| (start..at).contains(&r.instruction_index))
        || f.deferred_displacements
            .iter()
            .filter(|d| (start..at).contains(&d.instruction_index))
            .count()
            != symbols.len()
        || code.iter().any(|i| match i {
            I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                start < *target && *target < at
            }
            _ => false,
        })
    {
        return None;
    }
    // The already complete section base leads other narrow addresses, even if
    // source binding order puts its cursor last. Wide halves keep source order.
    if let Some(zero) = zero {
        let first = narrow.remove(zero);
        narrow.insert(0, first);
    }
    // Measured build-159 patch tie order: one high half with three independent
    // narrow addresses exchanges the leading ready pair. Two-high packets and
    // the unpatched profiles preserve the stable narrow order.
    let patched = f.patched_cursor_setup_order;
    if patched && high.len() == 1 && narrow.len() == 3 {
        narrow.swap(0, 1);
    }
    let completed_pair = high.len() == 1 && narrow.len() == 2 && complete_pages.len() == 1;
    if patched && completed_pair {
        narrow.sort_by_key(|at| !complete_pages.contains(at));
    }
    // The patched ready list places the index after the remaining low half
    // when this completed-page packet follows a call (including a save helper).
    let index_last = patched
        && completed_pair
        && code[..start].iter().any(|i| {
            matches!(
                i,
                I::BranchAndLink { .. }
                    | I::BranchToCountRegisterAndLink
                    | I::BranchToLinkRegisterAndLink
            )
        });
    high.extend(narrow);
    if !index_last {
        high.push(0);
    }
    high.extend(low);
    if index_last {
        high.push(0);
    }
    Some(Plan {
        old_length: at - start,
        order: high,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{DeferredDisplacement, Relocation, RelocationKind, RelocationTarget};

    fn fixture(offsets: &[u32]) -> (MachineFunction, Vec<(usize, u32)>) {
        let mut f = MachineFunction::new("cursor_setup");
        f.instructions.push(I::AddImmediate {
            d: 26,
            a: 0,
            immediate: 0,
        });
        let mut names = Vec::new();
        for (n, &offset) in offsets.iter().enumerate() {
            let destination = 30 - n as u8;
            let wide = i16::try_from(offset).is_err();
            if wide {
                f.instructions.push(I::AddImmediateShifted {
                    d: destination,
                    a: 31,
                    immediate: ((u64::from(offset) + 0x8000) >> 16) as i16,
                });
            }
            let name = format!("array{n}");
            f.deferred_displacements.push(DeferredDisplacement {
                instruction_index: f.instructions.len(),
                target: Target::Symbol(name.clone()),
            });
            f.instructions.push(I::AddImmediate {
                d: destination,
                a: if wide { destination } else { 31 },
                immediate: 0,
            });
            names.push(name);
        }
        f.symbol_order = names.clone();
        f.anchored_cursor_address_groups.push(names);
        f.instructions.push(I::StoreWord {
            s: 26,
            a: 27,
            offset: 0,
        });
        f.instructions.push(I::Branch { target: 0 });
        (f, offsets.iter().copied().enumerate().collect())
    }

    #[test]
    fn one_wide_cursor_selects_profile_order_and_preserves_fixup_owners() {
        for patched in [false, true] {
            let (mut f, offsets) = fixture(&[0, 640, 1280, 34048]);
            f.patched_cursor_setup_order = patched;
            let old = f.instructions.clone();
            let order = if patched {
                [4, 2, 1, 3, 0, 5]
            } else {
                [4, 1, 2, 3, 0, 5]
            };
            schedule(&mut f, &offsets);
            for (at, from) in order.into_iter().enumerate() {
                assert_eq!(f.instructions[at], old[from]);
            }
            for (d, original) in f.deferred_displacements.iter().zip([1, 2, 3, 5]) {
                assert_eq!(
                    d.instruction_index,
                    order.iter().position(|i| *i == original).unwrap()
                );
            }
            assert_eq!(f.instructions[6..], old[6..]);
            let once = f.instructions.clone();
            schedule(&mut f, &offsets);
            assert_eq!(f.instructions, once);
        }
    }

    #[test]
    fn two_wide_cursors_keep_half_order_with_both_profiles() {
        for patched in [false, true] {
            let (mut f, offsets) = fixture(&[0, 16384, 32768, 49152]);
            f.patched_cursor_setup_order = patched;
            let old = f.instructions.clone();
            schedule(&mut f, &offsets);
            for (at, from) in [3, 5, 1, 2, 0, 4, 6].into_iter().enumerate() {
                assert_eq!(f.instructions[at], old[from]);
            }
        }
    }

    #[test]
    fn zero_cursor_leads_narrow_bindings_and_three_cursor_groups_compose() {
        let (mut f, offsets) = fixture(&[34048, 1280, 640, 0]);
        let old = f.instructions.clone();
        schedule(&mut f, &offsets);
        for (at, from) in [1, 5, 3, 4, 0, 2].into_iter().enumerate() {
            assert_eq!(f.instructions[at], old[from]);
        }
        let (mut f, offsets) = fixture(&[32768, 0, 1280]);
        let old = f.instructions.clone();
        schedule(&mut f, &offsets);
        for (at, from) in [1, 3, 4, 0, 2].into_iter().enumerate() {
            assert_eq!(f.instructions[at], old[from]);
        }
    }

    #[test]
    fn completed_pages_drop_only_the_redundant_low_and_preserve_boundaries() {
        for patched in [false, true] {
            for called in [false, true] {
                let (mut f, mut offsets) = fixture(&[0, 32768, 65536]);
                f.patched_cursor_setup_order = patched;
                if called {
                    f.instructions.insert(
                        0,
                        I::BranchAndLink {
                            target: "before".into(),
                        },
                    );
                    for d in &mut f.deferred_displacements {
                        d.instruction_index += 1;
                    }
                    // The fixture's backedge enters the setup, after the call.
                    *f.instructions.last_mut().unwrap() = I::Branch { target: 1 };
                }
                let start = usize::from(called);
                let body = start + 6;
                f.instructions.push(I::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: body,
                });
                let extra = f.instructions.len();
                f.instructions.push(I::AddImmediate {
                    d: 3,
                    a: 31,
                    immediate: 0,
                });
                f.deferred_displacements.push(DeferredDisplacement {
                    instruction_index: extra,
                    target: Target::Symbol("extra".into()),
                });
                offsets.push((3, 12));
                f.relocations.push(Relocation {
                    instruction_index: extra + 1,
                    kind: RelocationKind::Rel24,
                    target: RelocationTarget::External("after".into()),
                });
                f.instructions.push(I::BranchAndLink {
                    target: "after".into(),
                });
                let old = f.instructions.clone();
                let expected = if patched && called {
                    [2, 4, 1, 3, 0]
                } else if patched {
                    [2, 4, 1, 0, 3]
                } else {
                    [2, 1, 4, 0, 3]
                };
                schedule(&mut f, &offsets);
                for (new, source) in expected.into_iter().enumerate() {
                    assert_eq!(f.instructions[start + new], old[start + source]);
                }
                assert_eq!(f.instructions.len(), old.len() - 1);
                assert_eq!(f.deferred_displacements.len(), 3);
                assert!(!f
                    .deferred_displacements
                    .iter()
                    .any(|d| matches!(&d.target, Target::Symbol(n) if n == "array2")));
                assert_eq!(
                    f.deferred_displacements.last().unwrap().instruction_index,
                    extra - 1
                );
                assert_eq!(f.relocations[0].instruction_index, extra);
                assert!(
                    matches!(f.instructions[body + 1], I::BranchConditionalForward { target, .. } if target == body - 1)
                );
                assert_eq!(f.instructions[body], I::Branch { target: start });
            }
        }
    }

    #[test]
    fn completed_page_symbols_must_survive_without_the_low_fixup() {
        let (mut f, offsets) = fixture(&[0, 32768, 65536]);
        f.symbol_order.retain(|name| name != "array2");
        let old = f.instructions.clone();
        schedule(&mut f, &offsets);
        assert_eq!(f.instructions, old);
        assert_eq!(f.deferred_displacements.len(), 3);
    }

    #[test]
    fn multiple_packets_are_proved_before_low_owners_are_removed() {
        let (mut f, mut offsets) = fixture(&[0, 32768, 65536]);
        let (mut second, more) = fixture(&[0, 32768, 65536]);
        let start = f.instructions.len();
        for d in &mut second.deferred_displacements {
            d.instruction_index += start;
        }
        *second.instructions.last_mut().unwrap() = I::Branch { target: start };
        f.instructions.extend(second.instructions);
        f.deferred_displacements
            .extend(second.deferred_displacements);
        offsets.extend(more.into_iter().map(|(fixup, offset)| (fixup + 3, offset)));
        let length = f.instructions.len();
        schedule(&mut f, &offsets);
        assert_eq!(f.instructions.len(), length - 2);
        assert_eq!(f.deferred_displacements.len(), 4);
        assert_eq!(
            &f.instructions[..start - 2],
            &f.instructions[start - 1..length - 3]
        );
        assert_eq!(
            f.instructions.last(),
            Some(&I::Branch { target: start - 1 })
        );
    }

    #[test]
    fn dependencies_and_interior_entries_are_rejected() {
        for variant in 0..5 {
            let (mut f, offsets) = fixture(&[0, 640, 1280, 34048]);
            match variant {
                0 => {
                    f.instructions[0] = I::AddImmediate {
                        d: 31,
                        a: 0,
                        immediate: 0,
                    }
                }
                1 => {
                    f.instructions[2] = I::AddImmediate {
                        d: 30,
                        a: 31,
                        immediate: 0,
                    }
                }
                2 => {
                    f.instructions[2] = I::AddImmediate {
                        d: 29,
                        a: 30,
                        immediate: 0,
                    }
                }
                3 => f.instructions.push(I::Branch { target: 2 }),
                _ => f.instructions.push(I::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 5,
                }),
            }
            let old = f.instructions.clone();
            schedule(&mut f, &offsets);
            assert_eq!(f.instructions, old);
        }
    }

    #[test]
    fn only_nominated_uniquely_owned_addresses_are_scheduled() {
        for variant in 0..7 {
            let (mut f, mut offsets) = fixture(&[0, 640, 1280, 34048]);
            match variant {
                0 => {
                    offsets.pop();
                }
                1 => f.anchored_cursor_address_groups.clear(),
                2 => f.anchored_cursor_address_groups[0][0] = "different".into(),
                3 => f
                    .deferred_displacements
                    .push(f.deferred_displacements[0].clone()),
                4 => f.relocations.push(Relocation {
                    instruction_index: 4,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::External("other".into()),
                }),
                5 => {
                    f.instructions[1] = I::AddImmediate {
                        d: 30,
                        a: 31,
                        immediate: 4,
                    }
                }
                _ => f.is_asm = true,
            }
            let old = f.instructions.clone();
            schedule(&mut f, &offsets);
            assert_eq!(f.instructions, old);
        }
    }
}
