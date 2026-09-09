//! Reuse word fields across disjoint member stores in the 4.1/4.3 optimizer.
//!
//! Addresses are relative to a source-proven ordinary pointer. Unknown stores,
//! overlapping byte ranges and control-flow entries end the proof. Pointed-to
//! byte loads are deliberately not cached: they may alias the stored members.
use crate::generator::Generator;
use mwcc_machine_code::Instruction as I;
use mwcc_syntax_trees::Type;
use mwcc_versions::Optimization;
use mwcc_vreg::{Class, RegisterRole};

fn has_operand(i: &I, r: u32, role: RegisterRole) -> bool {
    mwcc_vreg::register_operands(i)
        .iter()
        .any(|o| o.class == Class::General && o.register == r && o.role == role)
}

fn rename_uses(i: &mut I, old: u32, new: u32) {
    mwcc_vreg::for_each_register(i, |role, class, r| {
        if role == RegisterRole::Use && class == Class::General && *r == old {
            *r = new;
        }
    });
}

fn region_instruction(i: &I) -> bool {
    matches!(
        i,
        I::AddImmediate { .. }
            | I::Add { .. }
            | I::Or { .. }
            | I::ClearLeftImmediate { .. }
            | I::ClearLeftImmediateRecord { .. }
            | I::AndContiguousMask { .. }
            | I::AndMaskRecord { .. }
            | I::AndImmediateRecord { .. }
            | I::RotateAndMask { .. }
            | I::RotateAndMaskRecord { .. }
            | I::ShiftLeftImmediate { .. }
            | I::ShiftRightAlgebraicImmediate { .. }
            | I::CompareWordImmediate { .. }
            | I::CompareLogicalWordImmediate { .. }
            | I::LoadWord { .. }
            | I::LoadByteZero { .. }
            | I::LoadHalfwordZero { .. }
            | I::StoreWord { .. }
            | I::StoreByte { .. }
            | I::StoreHalfword { .. }
    )
}

fn store(i: &I) -> Option<(u32, i16, i32)> {
    match i {
        I::StoreWord { a, offset, .. } => Some((*a, *offset, 4)),
        I::StoreHalfword { a, offset, .. } => Some((*a, *offset, 2)),
        I::StoreByte { a, offset, .. } => Some((*a, *offset, 1)),
        _ => None,
    }
}

fn target(i: &I) -> Option<usize> {
    match i {
        I::Branch { target } | I::BranchConditionalForward { target, .. } => Some(*target),
        _ => None,
    }
}

struct Plan {
    previous: usize,
    source: u32,
    destination: u32,
    last_use: usize,
    // Retain a load's value in a virtual home before its scratch is overwritten.
    promote_until: Option<usize>,
}

fn plan(code: &[I], at: usize, bases: &[u32]) -> Option<Plan> {
    let I::LoadWord {
        d: destination,
        a: base,
        offset,
    } = code[at]
    else {
        return None;
    };
    if !bases.contains(&base) || destination == base || destination == 3 {
        return None;
    }
    // A parameter's original register ceases to prove an address after a write.
    if code[..at]
        .iter()
        .any(|i| has_operand(i, base, RegisterRole::Define))
    {
        return None;
    }
    let entries: Vec<_> = code.iter().filter_map(target).collect();
    let entry = entries
        .iter()
        .copied()
        .filter(|t| *t <= at)
        .max()
        .unwrap_or(0);
    let mut disjoint_store = false;
    let (previous, source, loaded) = (|| {
        for n in (0..at).rev() {
            let i = &code[n];
            if n < entry {
                return None;
            }
            match i {
                I::LoadWord { d, a, offset: o } if *a == base && *o == offset => {
                    return disjoint_store.then_some((n, *d, true));
                }
                I::StoreWord { s, a, offset: o } if *a == base && *o == offset => {
                    return disjoint_store.then_some((n, *s, false));
                }
                I::BranchConditionalForward { target, .. } if *target > at => continue,
                _ if !region_instruction(i) => return None,
                _ => {}
            }
            if let Some((a, o, width)) = store(i) {
                if a != base
                    || (i32::from(o) < i32::from(offset) + 4
                        && i32::from(offset) < i32::from(o) + width)
                {
                    return None;
                }
                disjoint_store = true;
            }
        }
        None
    })()?;
    if source == base {
        return None;
    }
    let overwritten =
        (previous + 1..at).find(|&n| has_operand(&code[n], source, RegisterRole::Define));
    let promote_until = if let Some(end) = overwritten {
        if !loaded
            || source == 3
            || code[previous + 1..=end]
                .iter()
                .any(|i| !region_instruction(i))
        {
            return None;
        }
        Some(end)
    } else {
        None
    };
    let mut end = at + 1;
    let mut last_use = at;
    let mut killed = false;
    while end < code.len() {
        if entries.contains(&end) || !region_instruction(&code[end]) {
            break;
        }
        if has_operand(&code[end], destination, RegisterRole::Use) {
            last_use = end;
        }
        if has_operand(&code[end], destination, RegisterRole::Define) {
            killed = true;
            break;
        }
        end += 1;
    }
    // Keep definitions whose value escapes the straight-line replacement run.
    if !killed && !crate::register_value_liveness::dead_after(code, end, destination) {
        return None;
    }
    if promote_until.is_none()
        && (at + 1..last_use).any(|n| has_operand(&code[n], source, RegisterRole::Define))
    {
        return None;
    }
    Some(Plan {
        previous,
        source,
        destination,
        last_use,
        promote_until,
    })
}

impl Generator {
    pub(crate) fn retain_disjoint_member_reads(&mut self, return_type: Type) {
        if !matches!(return_type, Type::Int | Type::UnsignedInt | Type::Void)
            || !self.behavior.retain_disjoint_member_reads
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
            || self.non_leaf
            || self.output.is_asm
            || !self.output.jump_tables.is_empty()
            || !self.output.entry_points.is_empty()
            || !self.output.relocations.is_empty()
            || !self.output.deferred_displacements.is_empty()
            || self
                .output
                .instructions
                .iter()
                .any(|i| matches!(i, I::VerbatimWord(_)))
        {
            return;
        }
        let bases = self
            .nonvolatile_pointer_bindings
            .iter()
            .filter_map(|name| self.locations.get(name).map(|l| l.register))
            .collect::<Vec<_>>();
        let mut at = 0;
        while at < self.output.instructions.len() {
            let Some(p) = plan(&self.output.instructions, at, &bases) else {
                at += 1;
                continue;
            };
            let source = if let Some(end) = p.promote_until {
                let fresh = self.fresh_virtual_general_preferring(
                    if self.behavior.schedule_latency_slots && self.behavior.scheduler_enabled {
                        4
                    } else {
                        5
                    },
                );
                if let I::LoadWord { d, .. } = &mut self.output.instructions[p.previous] {
                    *d = fresh;
                }
                for i in &mut self.output.instructions[p.previous + 1..=end] {
                    rename_uses(i, p.source, fresh);
                }
                fresh
            } else {
                p.source
            };
            for i in &mut self.output.instructions[at + 1..=p.last_use] {
                rename_uses(i, p.destination, source);
            }
            crate::remove_instruction_retargeting_to_next(self, at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reads() -> Vec<I> {
        vec![
            I::LoadWord {
                d: 4,
                a: 3,
                offset: 0,
            },
            I::LoadByteZero {
                d: 0,
                a: 4,
                offset: 0,
            },
            I::StoreByte {
                s: 0,
                a: 3,
                offset: 8,
            },
            I::LoadWord {
                d: 5,
                a: 3,
                offset: 0,
            },
            I::LoadByteZero {
                d: 0,
                a: 5,
                offset: 0,
            },
            I::BranchToLinkRegister,
        ]
    }
    #[test]
    fn only_disjoint_stores_through_a_proven_base_preserve_a_word() {
        assert!(plan(&reads(), 3, &[3]).is_some());
        assert!(plan(&reads(), 3, &[]).is_none());
        for offset in [-1, 0, 1, 2, 3] {
            let mut code = reads();
            code[2] = I::StoreHalfword { s: 0, a: 3, offset };
            assert!(plan(&code, 3, &[3]).is_none(), "offset {offset}");
        }
        let mut code = reads();
        code[2] = I::StoreByte {
            s: 0,
            a: 6,
            offset: 8,
        };
        assert!(plan(&code, 3, &[3]).is_none());
        code[2] = I::StoreHalfword {
            s: 0,
            a: 3,
            offset: -2,
        };
        assert!(plan(&code, 3, &[3]).is_some());
    }
    #[test]
    fn preserves_live_values_and_rejects_alternate_entries() {
        let mut code = reads();
        code[4] = I::AddImmediate {
            d: 4,
            a: 0,
            immediate: 1,
        };
        code.insert(
            5,
            I::LoadByteZero {
                d: 0,
                a: 5,
                offset: 0,
            },
        );
        assert!(plan(&code, 3, &[3]).is_none());
        let mut code = reads();
        code.push(I::Branch { target: 2 });
        assert!(plan(&code, 3, &[3]).is_none());
        let mut code = reads();
        code[4] = I::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        };
        code.push(I::Branch { target: 4 });
        assert!(plan(&code, 3, &[3]).is_none());
        let mut code = reads();
        code[5] = I::move_register(3, 5);
        assert!(plan(&code, 3, &[3]).is_some());
        code.insert(
            5,
            I::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 6,
            },
        );
        assert!(plan(&code, 3, &[3]).is_none());
    }
    #[test]
    fn retains_a_scratch_load_before_its_guard_destroys_the_scratch() {
        let code = vec![
            I::LoadWord {
                d: 0,
                a: 3,
                offset: 4,
            },
            I::AndMaskRecord {
                a: 0,
                s: 0,
                begin: 28,
                end: 31,
            },
            I::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            I::StoreByte {
                s: 6,
                a: 3,
                offset: 8,
            },
            I::LoadWord {
                d: 4,
                a: 3,
                offset: 4,
            },
            I::AddImmediate {
                d: 0,
                a: 4,
                immediate: 2,
            },
            I::StoreWord {
                s: 0,
                a: 3,
                offset: 4,
            },
            I::BranchToLinkRegister,
        ];
        let p = plan(&code, 4, &[3]).unwrap();
        assert_eq!(p.promote_until, Some(1));
        assert_eq!(p.last_use, 5);
    }
}
