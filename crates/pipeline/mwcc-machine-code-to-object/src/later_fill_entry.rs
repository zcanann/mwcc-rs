//! Schedule a quotient and first fill after temporary cursors and BSS addresses
//! have their final physical form. The source stage supplies nonvolatile-global
//! ownership; this stage proves the zero-offset address and scratch lifetimes.
use mwcc_machine_code::{
    DeferredDisplacementTarget, Instruction as I, MachineFunction, RelocationKind, RelocationTarget,
};
use mwcc_vreg::Class;

pub(super) fn schedule(f: &mut MachineFunction, resolved: &[(usize, u32)]) {
    let Some(policy) = f.later_fill_entry_schedule.take() else {
        return;
    };
    let Some(start) = f
        .instructions
        .iter()
        .position(|i| matches!(i, I::MoveToCountRegister { .. }))
        .and_then(|at| at.checked_sub(13))
    else {
        return;
    };
    if let Some(plan) = plan(
        f,
        start,
        &policy.anchor_symbol,
        |name| policy.published_globals.iter().any(|n| n == name),
        resolved,
        policy.add_immediate_copy,
        &policy.scalar_void_calls,
    ) {
        plan.apply(f, policy.add_immediate_copy);
    }
}

struct Plan {
    start: usize,
    body: usize,
    stores: usize,
    anchor: u8,
}
impl Plan {
    fn apply(self, f: &mut MachineFunction, copy: bool) {
        let s = self.start;
        if let I::AddImmediate { d, .. } = &mut f.instructions[s + 5] {
            *d = 4;
        }
        f.instructions[s + 6] = I::MultiplyHighWordUnsigned { d: 4, a: 4, b: 0 };
        if let I::ShiftRightLogicalImmediate { a, s, .. } = &mut f.instructions[s + 7] {
            *a = 4;
            *s = 4;
        }
        if let I::StoreWord { s, .. } = &mut f.instructions[s + 8] {
            *s = 4;
        }
        f.instructions[s + 11] = match copy {
            false => I::move_register(5, self.anchor),
            true => I::AddImmediate {
                d: 5,
                a: self.anchor,
                immediate: 0,
            },
        };
        for i in &mut f.instructions[self.body..self.body + self.stores] {
            if let I::StoreWord { a, .. } = i {
                *a = 5;
            }
        }
        f.instructions[self.body + self.stores] = I::AddImmediate {
            d: 5,
            a: 5,
            immediate: 4 * self.stores as i16,
        };
        if !copy {
            f.deferred_displacements
                .retain(|d| d.instruction_index != s + 11);
        }
        let order = [2, 0, 3, 4, 5, 1, 6, 9, 12, 10, 11, 7, 8, 13];
        let old = f.instructions[s..s + 14].to_vec();
        for (at, source) in order.iter().enumerate() {
            f.instructions[s + at] = old[*source].clone();
        }
        let remap = |at: usize| {
            if (s..s + 14).contains(&at) {
                s + order.iter().position(|n| *n == at - s).unwrap()
            } else {
                at
            }
        };
        for r in &mut f.relocations {
            r.instruction_index = remap(r.instruction_index);
        }
        for d in &mut f.deferred_displacements {
            d.instruction_index = remap(d.instruction_index);
        }
    }
}

fn plan(
    f: &MachineFunction,
    start: usize,
    anchor_symbol: &str,
    ordinary_global: impl Fn(&str) -> bool,
    resolved: &[(usize, u32)],
    add_immediate_copy: bool,
    scalar_void_calls: &[(String, u8)],
) -> Option<Plan> {
    let c = &f.instructions;
    let body = start + 14;
    let [I::AddImmediateShifted {
        d: 5,
        a: 0,
        immediate: 0,
    }, I::AddImmediate {
        d: anchor @ 14..=31,
        a: 5,
        immediate: 0,
    }, I::AddImmediateShifted { d: 4, a: 0, .. }, I::AddImmediateShifted { d: 3, a: 0, .. }, I::LoadWord { d: 0, a: 4, .. }, I::AddImmediate { d: 3, a: 3, .. }, I::MultiplyHighWordUnsigned { d: 0, a: 3, b: 0 }, I::ShiftRightLogicalImmediate { a: 0, s: 0, .. }, I::StoreWord {
        s: 0,
        a: 0,
        offset: 0,
    }, I::AddImmediate { d: 3, a: 0, .. }, I::StoreWord {
        s: 3,
        a: 0,
        offset: 0,
    }, I::AddImmediate {
        d: pointer,
        a: base,
        immediate: 0,
    }, I::AddImmediate {
        d: 0,
        a: 0,
        immediate: count,
    }, I::MoveToCountRegister { s: 0 }] = c.get(start..body)?
    else {
        return None;
    };
    if !(*pointer == 4 || (14..=31).contains(pointer))
        || pointer == anchor
        || anchor != base
        || *count < 2
        || f.is_asm
        || !f.entry_points.is_empty()
        || !f.jump_tables.is_empty()
        || c.iter().any(|i| {
            matches!(
                i,
                I::VerbatimWord(_)
                    | I::BranchToCountRegister
                    | I::SystemCall
                    | I::ReturnFromInterrupt
            )
        })
    {
        return None;
    }
    let stores = c[body..]
        .iter()
        .take(10)
        .enumerate()
        .take_while(|(n, i)| matches!(i,I::StoreWord { s:3,a,offset } if a==pointer && *offset==4*(*n as i16)))
        .count();
    if ![8, 10].contains(&stores)
        || !matches!(c.get(body+stores), Some(I::AddImmediate { d,a,immediate }) if d==pointer && a==pointer && *immediate==4*stores as i16)
        || !matches!(c.get(body+stores+1),Some(I::BranchConditionalForward { options:16,condition_bit:0,target }) if *target==body)
    {
        return None;
    }
    let end = body + stores + 2;
    if c.iter().enumerate().any(|(at, i)| match i {
        I::Branch { target } | I::BranchConditionalForward { target, .. } => {
            (start + 1..end).contains(target) && !(at == end - 1 && *target == body)
        }
        _ => false,
    }) {
        return None;
    }
    let rel: Vec<_> = f
        .relocations
        .iter()
        .filter(|r| (start..end).contains(&r.instruction_index))
        .collect();
    if rel.len() != 4 {
        return None;
    }
    let symbol = |at, kind| {
        rel.iter().find_map(|r| {
            if r.instruction_index != at || r.kind != kind {
                return None;
            }
            if let RelocationTarget::External(name) = &r.target {
                Some(name.as_str())
            } else {
                None
            }
        })
    };
    if symbol(start, RelocationKind::Addr16Ha)? != anchor_symbol
        || symbol(start + 1, RelocationKind::Addr16Lo)? != anchor_symbol
    {
        return None;
    }
    let result = symbol(start + 8, RelocationKind::EmbSda21)?;
    let fill = symbol(start + 10, RelocationKind::EmbSda21)?;
    if result == fill || !ordinary_global(result) || !ordinary_global(fill) {
        return None;
    }
    let displacements: Vec<_> = f
        .deferred_displacements
        .iter()
        .filter(|d| (start..end).contains(&d.instruction_index))
        .collect();
    if displacements.len() != 1
        || displacements[0].instruction_index != start + 11
        || !resolved.iter().any(|(at, offset)| {
            *offset == 0 && f.deferred_displacements[*at].instruction_index == start + 11
        })
        || !matches!(&displacements[0].target,DeferredDisplacementTarget::SymbolAddress(n) | DeferredDisplacementTarget::Symbol(n) if add_immediate_copy || f.symbol_order.contains(n))
    {
        return None;
    }
    // Only inspect the fallthrough exit: the backedge deliberately carries the
    // recolored cursor. Neither that old cursor nor the replaced r4/r5 values
    // may escape. Known void calls kill scratch lanes beyond their arguments;
    // unknown calls retain the conservative ABI liveness model.
    for register in [4, 5, *pointer] {
        let mut proof = c.to_vec();
        if register <= 12 {
            for i in &mut proof {
                if matches!(i,I::BranchAndLink { target } if scalar_void_calls.iter().any(|(name,count)| name==target && register>=3+count))
                {
                    *i = I::load_immediate(register, 0);
                }
            }
        }
        let live = mwcc_vreg::analyze(&proof);
        if live.pinned.iter().any(|r| {
            r.class == Class::General
                && r.register == register
                && r.live_slots
                    .as_ref()
                    .is_some_and(|slots| slots.binary_search(&(2 * end)).is_ok())
        }) {
            return None;
        }
    }
    Some(Plan {
        start,
        body,
        stores,
        anchor: *anchor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{DeferredDisplacement, Relocation};

    fn fixture(stores: usize) -> MachineFunction {
        let mut f = MachineFunction::new("fill");
        f.instructions = vec![
            I::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            I::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            I::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: -32768,
            },
            I::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0x51ec,
            },
            I::LoadWord {
                d: 0,
                a: 4,
                offset: 248,
            },
            I::AddImmediate {
                d: 3,
                a: 3,
                immediate: -31457,
            },
            I::MultiplyHighWordUnsigned { d: 0, a: 3, b: 0 },
            I::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 7,
            },
            I::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            I::load_immediate(3, 0),
            I::StoreWord {
                s: 3,
                a: 0,
                offset: 0,
            },
            I::AddImmediate {
                d: 4,
                a: 31,
                immediate: 0,
            },
            I::load_immediate(0, 8),
            I::MoveToCountRegister { s: 0 },
        ];
        for n in 0..stores {
            f.instructions.push(I::StoreWord {
                s: 3,
                a: 4,
                offset: 4 * n as i16,
            });
        }
        f.instructions.extend([
            I::AddImmediate {
                d: 4,
                a: 4,
                immediate: 4 * stores as i16,
            },
            I::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 14,
            },
            I::load_immediate(4, 99),
            I::load_immediate(5, 99),
            I::BranchToLinkRegister,
        ]);
        for (instruction_index, kind, name) in [
            (0, RelocationKind::Addr16Ha, "anchor"),
            (1, RelocationKind::Addr16Lo, "anchor"),
            (8, RelocationKind::EmbSda21, "result"),
            (10, RelocationKind::EmbSda21, "fill"),
        ] {
            f.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External(name.into()),
            });
        }
        f.deferred_displacements.push(DeferredDisplacement {
            instruction_index: 11,
            target: DeferredDisplacementTarget::SymbolAddress("array".into()),
        });
        f.symbol_order.push("array".into());
        f
    }
    fn selected(f: &MachineFunction, addi: bool) -> Option<Plan> {
        plan(
            f,
            0,
            "anchor",
            |n| ["result", "fill"].contains(&n),
            &[(0, 0)],
            addi,
            &[],
        )
    }
    #[test]
    fn schedules_both_fill_widths_and_preserves_metadata_owners() {
        for stores in [8, 10] {
            for addi in [false, true] {
                let mut f = fixture(stores);
                let tail = f.instructions[14 + stores + 2..].to_vec();
                selected(&f, addi).unwrap().apply(&mut f, addi);
                assert!(matches!(
                    f.instructions[0],
                    I::AddImmediateShifted { d: 4, .. }
                ));
                assert!(matches!(
                    f.instructions[4],
                    I::AddImmediate { d: 4, a: 3, .. }
                ));
                assert!(matches!(
                    f.instructions[6],
                    I::MultiplyHighWordUnsigned { d: 4, a: 4, b: 0 }
                ));
                assert!(
                    matches!(
                        f.instructions[10],
                        I::AddImmediate {
                            d: 5,
                            a: 31,
                            immediate: 0
                        }
                    ) == addi
                );
                assert!(matches!(
                    f.instructions[11],
                    I::ShiftRightLogicalImmediate {
                        a: 4,
                        s: 4,
                        shift: 7
                    }
                ));
                assert_eq!(
                    f.relocations
                        .iter()
                        .map(|r| r.instruction_index)
                        .collect::<Vec<_>>(),
                    [1, 5, 12, 9]
                );
                assert_eq!(f.deferred_displacements.len(), usize::from(addi));
                if addi {
                    assert_eq!(f.deferred_displacements[0].instruction_index, 10);
                }
                for i in &f.instructions[14..14 + stores] {
                    assert!(matches!(i, I::StoreWord { s: 3, a: 5, .. }));
                }
                assert_eq!(f.instructions[14 + stores + 2..], tail);
                assert!(selected(&f, addi).is_none());
            }
        }
    }
    #[test]
    fn publication_ownership_and_volatility_are_required() {
        let f = fixture(8);
        assert!(plan(&f, 0, "anchor", |n| n == "result", &[(0, 0)], true, &[]).is_none());
        let mut f = f.clone();
        f.relocations[3].target = RelocationTarget::External("result".into());
        assert!(selected(&f, true).is_none());
    }
    #[test]
    fn only_a_proven_zero_address_can_become_an_anchor_copy() {
        let mut f = fixture(8);
        assert!(plan(&f, 0, "anchor", |_| true, &[(0, 4)], true, &[]).is_none());
        assert!(plan(&f, 0, "anchor", |_| true, &[], true, &[]).is_none());
        f.symbol_order.clear();
        assert!(selected(&f, false).is_none());
        assert!(selected(&f, true).is_some());
        f.relocations[0].target = RelocationTarget::External("wrong".into());
        assert!(selected(&f, true).is_none());
    }
    #[test]
    fn live_cursor_values_and_live_replacements_prevent_recoloring() {
        for reg in [4, 5] {
            let mut f = fixture(8);
            f.instructions.insert(
                24,
                I::StoreWord {
                    s: reg,
                    a: 31,
                    offset: 64,
                },
            );
            assert!(selected(&f, true).is_none());
        }
        let mut f = fixture(8);
        f.instructions[24] = I::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 27,
        };
        f.instructions.push(I::StoreWord {
            s: 4,
            a: 31,
            offset: 64,
        });
        assert!(selected(&f, true).is_none());
    }
    #[test]
    fn saved_fill_cursor_is_replaced_only_when_its_exit_value_is_dead() {
        let mut f = fixture(8);
        f.instructions[11] = I::AddImmediate {
            d: 30,
            a: 31,
            immediate: 0,
        };
        for i in &mut f.instructions[14..22] {
            if let I::StoreWord { a, .. } = i {
                *a = 30;
            }
        }
        f.instructions[22] = I::AddImmediate {
            d: 30,
            a: 30,
            immediate: 32,
        };
        f.instructions.insert(24, I::load_immediate(30, 17));
        assert!(selected(&f, true).is_some());
        f.instructions[24] = I::StoreWord {
            s: 30,
            a: 31,
            offset: 88,
        };
        assert!(selected(&f, true).is_none());
    }
    #[test]
    fn known_void_call_argument_counts_bound_scratch_lifetimes() {
        let mut f = fixture(8);
        f.instructions[25] = I::BranchAndLink {
            target: "callback".into(),
        };
        assert!(selected(&f, true).is_none());
        assert!(plan(
            &f,
            0,
            "anchor",
            |_| true,
            &[(0, 0)],
            true,
            &[("callback".into(), 1)]
        )
        .is_some());
        assert!(plan(
            &f,
            0,
            "anchor",
            |_| true,
            &[(0, 0)],
            true,
            &[("callback".into(), 3)]
        )
        .is_none());
        assert!(plan(
            &f,
            0,
            "anchor",
            |_| true,
            &[(0, 0)],
            true,
            &[("other".into(), 1)]
        )
        .is_none());
    }
    #[test]
    fn malformed_loops_and_interior_entries_are_barriers() {
        for variant in 0..8 {
            let mut f = fixture(8);
            match variant {
                0 => {
                    f.instructions[14] = I::StoreWord {
                        s: 0,
                        a: 4,
                        offset: 0,
                    }
                }
                1 => {
                    f.instructions[22] = I::AddImmediate {
                        d: 4,
                        a: 4,
                        immediate: 40,
                    }
                }
                2 => f.instructions.push(I::Branch { target: 11 }),
                3 => f.instructions.push(I::Branch { target: 14 }),
                4 => f.entry_points.push(("entry".into(), 0)),
                6 => f.instructions.push(I::SystemCall),
                7 => f.instructions.push(I::ReturnFromInterrupt),
                _ => f.relocations.push(Relocation {
                    instruction_index: 15,
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External("fill".into()),
                }),
            }
            assert!(selected(&f, true).is_none());
        }
    }
}
