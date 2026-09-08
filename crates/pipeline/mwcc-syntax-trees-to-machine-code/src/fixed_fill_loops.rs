//! Constant-trip pointer fills on the selected, unallocated instruction graph.
//!
//! A single-entry countdown loop proves the trip count without recovering a
//! source template. The body has one constant store and one pointer advance.
//! Version policy selects its expansion; ordinary liveness retains observable
//! exit values and allocation chooses the new CTR source home.

use crate::generator::Generator;
use mwcc_machine_code::Instruction as I;
use mwcc_syntax_trees::{GlobalDeclaration, Type};
use mwcc_versions::{FixedFillLoopStyle, Optimization, OptimizationGoal};
use mwcc_vreg::{Class, Liveness, Reg};

#[derive(Debug)]
struct Plan {
    start: usize,
    counter: u8,
    pointer: u8,
    fill: u8,
    value: i16,
    count: i16,
    width: i16,
    store: I,
    factor: i16,
    keep_counter: bool,
    keep_pointer: bool,
}

impl Plan {
    fn store_at(&self, slot: i16) -> I {
        let mut store = self.store.clone();
        match &mut store {
            I::StoreWord { offset, .. }
            | I::StoreHalfword { offset, .. }
            | I::StoreByte { offset, .. } => *offset = slot * self.width,
            _ => unreachable!(),
        }
        store
    }
}

impl Generator {
    pub(crate) fn expand_fixed_fill_loops(
        &mut self,
        return_type: Type,
        globals: &[GlobalDeclaration],
    ) {
        if self.behavior.optimization < Optimization::O3
            || self.output.is_asm
            || !self.output.entry_points.is_empty()
            || !self.output.jump_tables.is_empty()
            || self.output.instructions.iter().any(|i| {
                matches!(
                    i,
                    I::VerbatimWord(_)
                        | I::MoveToCountRegister { .. }
                        | I::BranchToCountRegister
                        | I::BranchToCountRegisterAndLink
                        | I::BranchConditionalForward {
                            options: 16..=19,
                            ..
                        }
                )
            })
        {
            return;
        }
        // GC 3/Wii revert to the ten-store divisor policy for an entire
        // function that references an explicitly aligned global, even if the
        // fill itself writes through an unrelated parameter. Unreferenced
        // aligned declarations do not affect it (alignment 4 also triggers).
        let aligned_reference = globals.iter().any(|g| {
            g.attribute_alignment.is_some()
                && (self.output.symbol_order.contains(&g.name)
                    || self.output.relocations.iter().any(|r| match &r.target {
                        mwcc_machine_code::RelocationTarget::External(n)
                        | mwcc_machine_code::RelocationTarget::ExternalWithAddend(n, _) => {
                            n == &g.name
                        }
                        _ => false,
                    })
                    || self
                        .output
                        .deferred_displacements
                        .iter()
                        .any(|d| match &d.target {
                            mwcc_machine_code::DeferredDisplacementTarget::Symbol(n)
                            | mwcc_machine_code::DeferredDisplacementTarget::SymbolAddress(n) => {
                                n == &g.name
                            }
                            _ => false,
                        }))
        });
        let style = if aligned_reference {
            FixedFillLoopStyle::DivisorTen
        } else {
            self.behavior.fixed_fill_loop_style
        };
        let live = mwcc_vreg::analyze(&self.output.instructions);
        let plans: Vec<_> = (0..self.output.instructions.len())
            .filter_map(|start| {
                recognize(
                    &self.output,
                    start,
                    &live,
                    return_type,
                    style,
                    self.behavior.optimization_goal,
                )
            })
            .collect();
        for plan in plans.into_iter().rev() {
            let trip = self.fresh_virtual_general_preferring(0);
            let mut code = Vec::new();
            let iterations = plan.count / plan.factor;
            if iterations > 1 {
                code.push(I::load_immediate(trip, iterations));
                code.push(I::MoveToCountRegister { s: trip });
            }
            code.push(I::load_immediate(plan.fill, plan.value));
            let body = code.len();
            for slot in 0..plan.factor {
                code.push(plan.store_at(slot));
            }
            if iterations > 1 || plan.keep_pointer {
                code.push(I::AddImmediate {
                    d: plan.pointer,
                    a: plan.pointer,
                    immediate: plan.factor * plan.width,
                });
            }
            let branch = if iterations > 1 {
                let at = code.len();
                code.push(I::BranchConditionalForward {
                    options: 16,
                    condition_bit: 0,
                    target: 0,
                });
                Some(at)
            } else {
                None
            };
            let remainder = plan.count % plan.factor;
            if remainder != 0 {
                code.push(I::load_immediate(plan.fill, plan.value));
                for slot in 0..remainder {
                    code.push(plan.store_at(slot));
                }
                if plan.keep_pointer {
                    code.push(I::AddImmediate {
                        d: plan.pointer,
                        a: plan.pointer,
                        immediate: remainder * plan.width,
                    });
                }
            }
            if plan.keep_counter {
                code.push(I::load_immediate(plan.counter, 0));
            }
            // Keep the entry instruction in place: external entries still run
            // the setup. All other old destinations were excluded by the proof.
            self.output.instructions[plan.start] = code[0].clone();
            for _ in 1..7 {
                crate::remove_instruction_retargeting_to_next(self, plan.start + 1);
            }
            for (index, instruction) in code.into_iter().enumerate().skip(1) {
                crate::insert_instruction_retargeting(self, plan.start + index, instruction);
            }
            if let Some(branch) = branch {
                if let I::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[plan.start + branch]
                {
                    *target = plan.start + body;
                }
            }
        }
    }
}

fn factor(count: i16, style: FixedFillLoopStyle, goal: OptimizationGoal) -> Option<i16> {
    if count <= 0 {
        return None;
    }
    if goal == OptimizationGoal::Size {
        return Some(1);
    }
    match style {
        FixedFillLoopStyle::DivisorTen => (1..=10).rev().find(|n| count % n == 0),
        FixedFillLoopStyle::PacketEight if count < 64 => Some(count),
        FixedFillLoopStyle::PacketEight => {
            // Divide the complete eight-store packets; at most seven scalar
            // stores remain after the CTR loop (100 => 2 * 48 + 4).
            let packets = count / 8;
            (1..=7).rev().find(|n| packets % n == 0).map(|n| n * 8)
        }
    }
}

fn recognize(
    function: &mwcc_machine_code::MachineFunction,
    start: usize,
    live: &Liveness,
    return_type: Type,
    style: FixedFillLoopStyle,
    goal: OptimizationGoal,
) -> Option<Plan> {
    let code = &function.instructions;
    let [I::AddImmediate {
        d: counter,
        a: 0,
        immediate: count,
    }, I::AddImmediate {
        d: fill,
        a: 0,
        immediate: value,
    }, store, I::AddImmediate {
        d: pointer,
        a: base,
        immediate: width,
    }, I::AddImmediate {
        d: down,
        a: previous,
        immediate: -1,
    }, compare, I::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target,
    }] = code.get(start..start + 7)?
    else {
        return None;
    };
    if pointer != base
        || counter != down
        || counter != previous
        || *target != start + 1
        || pointer == counter
        || pointer == fill
        || counter == fill
        || *pointer == 0
        || !matches!(compare,
            I::CompareWordImmediate { a, immediate: 0 }
            | I::CompareLogicalWordImmediate { a, immediate: 0 } if a == counter)
    {
        return None;
    }
    let (source, address, size) = match store {
        I::StoreWord { s, a, offset: 0 } => (s, a, 4),
        I::StoreHalfword { s, a, offset: 0 } => (s, a, 2),
        I::StoreByte { s, a, offset: 0 } => (s, a, 1),
        _ => return None,
    };
    if source != fill || address != pointer || *width != size {
        return None;
    }
    if function
        .relocations
        .iter()
        .any(|r| (start..start + 7).contains(&r.instruction_index))
        || function
            .deferred_displacements
            .iter()
            .any(|d| (start..start + 7).contains(&d.instruction_index))
        || code.iter().enumerate().any(|(at, i)| match i {
            I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                at != start + 6 && (start + 1..start + 7).contains(target)
            }
            _ => false,
        })
        || !cr0_dead_at(code, start + 7)
    {
        return None;
    }
    let keep = |register| {
        live_at(live, register, start + 7) || (register == 3 && return_type != Type::Void)
    };
    Some(Plan {
        start,
        counter: *counter,
        pointer: *pointer,
        fill: *fill,
        value: *value,
        count: *count,
        width: *width,
        store: store.clone(),
        factor: factor(*count, style, goal)?,
        keep_counter: keep(*counter),
        keep_pointer: keep(*pointer),
    })
}

fn live_at(live: &Liveness, register: u8, at: usize) -> bool {
    let slots = if let Some(vreg) = Reg::from_field(register, Class::General).virtual_register() {
        live.intervals
            .iter()
            .find(|i| i.vreg == vreg)
            .and_then(|i| i.live_slots.as_ref())
    } else {
        live.pinned
            .iter()
            .find(|p| p.class == Class::General && p.register == register)
            .and_then(|p| p.live_slots.as_ref())
    };
    slots.is_some_and(|slots| slots.binary_search(&(2 * at)).is_ok())
}

/// Prove the removed final comparison cannot feed a later condition. Unknown
/// CR operations conservatively stop the proof; calls kill volatile CR0.
fn cr0_dead_at(code: &[I], mut at: usize) -> bool {
    let mut seen = std::collections::HashSet::new();
    while let Some(i) = code.get(at) {
        if !seen.insert(at) {
            return false;
        }
        match i {
            I::CompareWord { .. }
            | I::CompareWordImmediate { .. }
            | I::CompareLogicalWord { .. }
            | I::CompareLogicalWordImmediate { .. }
            | I::BranchAndLink { .. }
            | I::BranchToLinkRegister => return true,
            I::Branch { target } => {
                at = *target;
                continue;
            }
            I::BranchConditionalForward { .. }
            | I::BranchConditionalToLinkRegister { .. }
            | I::MoveFromConditionRegister { .. }
            | I::MoveToConditionRegisterFields { .. }
            | I::ConditionRegisterOr { .. }
            | I::ConditionRegisterSet { .. }
            | I::ConditionRegisterClear { .. }
            | I::BranchExternal { .. } => return false,
            _ => {}
        }
        at += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation, RelocationKind, RelocationTarget};

    fn function() -> MachineFunction {
        let mut f = MachineFunction::new("fill");
        f.instructions = vec![
            I::load_immediate(4, 32),
            I::load_immediate(0, 7),
            I::StoreWord {
                s: 0,
                a: 3,
                offset: 0,
            },
            I::AddImmediate {
                d: 3,
                a: 3,
                immediate: 4,
            },
            I::AddImmediate {
                d: 4,
                a: 4,
                immediate: -1,
            },
            I::CompareLogicalWordImmediate { a: 4, immediate: 0 },
            I::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 1,
            },
            I::BranchToLinkRegister,
        ];
        f
    }

    fn plan(f: &MachineFunction, result: Type) -> Option<Plan> {
        recognize(
            f,
            0,
            &mwcc_vreg::analyze(&f.instructions),
            result,
            FixedFillLoopStyle::DivisorTen,
            OptimizationGoal::Performance,
        )
    }

    #[test]
    fn factors_follow_trip_divisibility_and_size_objective() {
        for (count, expected) in [
            (1, 1),
            (10, 10),
            (11, 1),
            (12, 6),
            (15, 5),
            (16, 8),
            (18, 9),
            (32, 8),
            (40, 10),
            (3904, 8),
            (8896, 8),
        ] {
            assert_eq!(
                factor(
                    count,
                    FixedFillLoopStyle::DivisorTen,
                    OptimizationGoal::Performance
                ),
                Some(expected)
            );
            assert_eq!(
                factor(
                    count,
                    FixedFillLoopStyle::DivisorTen,
                    OptimizationGoal::Size
                ),
                Some(1)
            );
        }
        for (count, expected) in [
            (32, 32),
            (63, 63),
            (64, 32),
            (72, 24),
            (120, 40),
            (168, 56),
            (3904, 32),
        ] {
            assert_eq!(
                factor(
                    count,
                    FixedFillLoopStyle::PacketEight,
                    OptimizationGoal::Performance
                ),
                Some(expected)
            );
        }
        assert_eq!(
            factor(
                100,
                FixedFillLoopStyle::PacketEight,
                OptimizationGoal::Performance
            ),
            Some(48)
        );
        assert_eq!(
            factor(0, FixedFillLoopStyle::DivisorTen, OptimizationGoal::Size),
            None
        );
    }

    #[test]
    fn retains_exit_counter_and_returned_pointer_values() {
        let mut f = function();
        assert!(!plan(&f, Type::Void).unwrap().keep_counter);
        assert!(!plan(&f, Type::Void).unwrap().keep_pointer);
        assert!(plan(&f, Type::UnsignedInt).unwrap().keep_pointer);
        f.instructions.insert(7, I::move_register(3, 4));
        assert!(plan(&f, Type::UnsignedInt).unwrap().keep_counter);
    }

    #[test]
    fn rejects_extra_entries_fixups_and_observable_condition_codes() {
        let mut f = function();
        f.instructions.push(I::Branch { target: 2 });
        assert!(plan(&f, Type::Void).is_none());
        f.instructions.pop();
        f.relocations.push(Relocation {
            instruction_index: 2,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External("buffer".into()),
        });
        assert!(plan(&f, Type::Void).is_none());
        f.relocations.clear();
        f.instructions
            .insert(7, I::MoveFromConditionRegister { d: 3 });
        assert!(plan(&f, Type::UnsignedInt).is_none());
    }

    #[test]
    fn rejects_zero_trip_and_aliasing_loop_state() {
        let mut f = function();
        f.instructions[0] = I::load_immediate(4, 0);
        assert!(plan(&f, Type::Void).is_none());
        f = function();
        f.instructions[2] = I::StoreWord {
            s: 4,
            a: 3,
            offset: 0,
        };
        assert!(plan(&f, Type::Void).is_none());
    }
}
