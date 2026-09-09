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
    counter: u32,
    pointer: u32,
    fill: u32,
    value: i16,
    count: i16,
    width: i16,
    store: I,
    factor: i16,
    keep_counter: bool,
    keep_pointer: bool,
}

impl Plan {
    fn store_at(&self, slot: i16, fill: u32) -> I {
        let mut store = self.store.clone();
        match &mut store {
            I::StoreWord { s, offset, .. }
            | I::StoreHalfword { s, offset, .. }
            | I::StoreByte { s, offset, .. } => {
                *s = fill;
                *offset = slot * self.width;
            }
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
            let retained = retained_fill_prefix(&self.output, &plan, &live);
            let fill = if let Some(load) = retained {
                let fill = self.fresh_virtual_general_preferring(3);
                let I::AddImmediate { d, .. } = &mut self.output.instructions[load] else {
                    unreachable!("the retained fill owns its literal");
                };
                *d = fill;
                for instruction in &mut self.output.instructions[load + 1..plan.start] {
                    mwcc_vreg::for_each_register(instruction, |role, class, register| {
                        if role == mwcc_vreg::RegisterRole::Use
                            && class == Class::General
                            && *register == plan.fill
                        {
                            *register = fill;
                        }
                    });
                }
                fill
            } else {
                plan.fill
            };
            let trip = self.fresh_virtual_general_preferring(0);
            let mut code = Vec::new();
            let iterations = plan.count / plan.factor;
            if iterations > 1 {
                code.push(I::load_immediate(trip, iterations));
                code.push(I::MoveToCountRegister { s: trip });
            }
            if retained.is_none() {
                code.push(I::load_immediate(fill, plan.value));
            }
            let body = code.len();
            for slot in 0..plan.factor {
                code.push(plan.store_at(slot, fill));
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
                code.push(I::load_immediate(fill, plan.value));
                for slot in 0..remainder {
                    code.push(plan.store_at(slot, fill));
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

/// Retain a scratch literal whose definition dominates the complete loop.
/// The prefix must be straight-line and must not redefine the scratch. A fresh
/// virtual value lets allocation keep it alive while the CTR source is issued.
fn retained_fill_prefix(
    function: &mwcc_machine_code::MachineFunction,
    plan: &Plan,
    live: &Liveness,
) -> Option<usize> {
    if plan.fill != 0 || live_at(live, plan.fill, plan.start + 7) {
        return None;
    }
    for at in (0..plan.start).rev() {
        let instruction = &function.instructions[at];
        // Limit the prefix to ordinary address setup and memory operations.
        // Unknown control flow, implicit definitions, and calls end the proof.
        if !matches!(
            instruction,
            I::AddImmediate { .. }
                | I::AddImmediateShifted { .. }
                | I::LoadWord { .. }
                | I::LoadHalfwordZero { .. }
                | I::LoadByteZero { .. }
                | I::StoreWord { .. }
                | I::StoreHalfword { .. }
                | I::StoreByte { .. }
        ) {
            return None;
        }
        if mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| {
                operand.class == Class::General
                    && operand.role == mwcc_vreg::RegisterRole::Define
                    && operand.register == plan.fill
            })
        {
            if !matches!(instruction, I::AddImmediate { d: 0, a: 0, immediate }
                if *immediate == plan.value)
                || function
                    .relocations
                    .iter()
                    .any(|r| r.instruction_index == at)
                || function
                    .deferred_displacements
                    .iter()
                    .any(|r| r.instruction_index == at)
                || function.instructions.iter().any(|i| {
                    matches!(i,
                    I::Branch { target } | I::BranchConditionalForward { target, .. }
                        if (at + 1..plan.start + 1).contains(target))
                })
            {
                return None;
            }
            return Some(at);
        }
    }
    None
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

fn live_at(live: &Liveness, register: u32, at: usize) -> bool {
    let slots = if let Some(vreg) = Reg::from_field(register, Class::General).virtual_register() {
        live.intervals
            .iter()
            .find(|i| i.vreg == vreg)
            .and_then(|i| i.live_slots.as_ref())
    } else {
        live.pinned
            .iter()
            .find(|p| p.class == Class::General && u32::from(p.register) == register)
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

    fn prefixed_function() -> (MachineFunction, Plan) {
        let mut f = function();
        f.instructions.splice(
            0..0,
            [
                I::load_immediate(0, 7),
                I::StoreWord {
                    s: 0,
                    a: 5,
                    offset: 0,
                },
                I::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 4,
                },
            ],
        );
        if let I::BranchConditionalForward { target, .. } = &mut f.instructions[9] {
            *target += 3;
        }
        let plan = recognize(
            &f,
            3,
            &mwcc_vreg::analyze(&f.instructions),
            Type::Void,
            FixedFillLoopStyle::DivisorTen,
            OptimizationGoal::Performance,
        )
        .unwrap();
        (f, plan)
    }

    #[test]
    fn retains_a_dominating_prefix_literal_across_address_setup() {
        let (f, plan) = prefixed_function();
        assert_eq!(
            retained_fill_prefix(&f, &plan, &mwcc_vreg::analyze(&f.instructions)),
            Some(0)
        );
    }

    #[test]
    fn rejects_prefix_bypasses_calls_and_redefinitions() {
        let (mut f, plan) = prefixed_function();
        f.instructions.push(I::Branch { target: plan.start });
        assert!(retained_fill_prefix(&f, &plan, &mwcc_vreg::analyze(&f.instructions)).is_none());
        for instruction in [
            I::load_immediate(0, 9),
            I::BranchAndLink {
                target: "observe".into(),
            },
        ] {
            let (mut f, plan) = prefixed_function();
            f.instructions[2] = instruction;
            assert!(
                retained_fill_prefix(&f, &plan, &mwcc_vreg::analyze(&f.instructions)).is_none()
            );
        }
    }

    #[test]
    fn preserves_symbolic_literals_and_observable_exit_scratch() {
        let (mut f, plan) = prefixed_function();
        f.deferred_displacements
            .push(mwcc_machine_code::DeferredDisplacement {
                instruction_index: 0,
                target: mwcc_machine_code::DeferredDisplacementTarget::SymbolAddress(
                    "global".into(),
                ),
            });
        assert!(retained_fill_prefix(&f, &plan, &mwcc_vreg::analyze(&f.instructions)).is_none());
        f.deferred_displacements.clear();
        f.instructions
            .insert(plan.start + 7, I::move_register(3, 0));
        assert!(retained_fill_prefix(&f, &plan, &mwcc_vreg::analyze(&f.instructions)).is_none());
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
