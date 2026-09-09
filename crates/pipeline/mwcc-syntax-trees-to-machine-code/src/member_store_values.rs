//! Retain values across member-initialization runs before allocation.
//!
//! Constants and interior pointers share one store-value graph; version policies
//! select the value issue order. Stores retain their original order, including
//! volatile writes and assignment chains. Physical homes come from liveness.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_versions::{MemberValueSchedule, Optimization};
use mwcc_vreg::{Class, Reg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    Constant(i16),
    Address(i16),
}

#[derive(Debug)]
struct Plan {
    base: u32,
    values: Vec<Value>,
    // None keeps an independent store's existing register source.
    stores: Vec<(Instruction, Option<usize>)>,
}

impl Generator {
    pub(crate) fn retain_member_store_values(&mut self) {
        if !matches!(
            self.behavior.optimization,
            Optimization::O2 | Optimization::O3 | Optimization::O4
        ) {
            return;
        }
        let leaf = self.output.relocations.is_empty()
            && self.output.entry_points.is_empty()
            && self.output.jump_tables.is_empty()
            && self.output.deferred_displacements.is_empty();
        if let Some(plan) = leaf.then(|| plan(&self.output.instructions)).flatten() {
            self.output.instructions = self.emit_member_values(&plan).0;
        } else {
            self.retain_member_store_regions();
        }
    }

    fn emit_member_values(&mut self, plan: &Plan) -> (Vec<Instruction>, Vec<u32>) {
        let registers: Vec<_> = plan
            .values
            .iter()
            .map(|_| self.fresh_virtual_general_preferring(0))
            .collect();
        // Reverse consumer order gives the last value first choice of scratch.
        // Interference protects the member base and all overlapping values.
        self.consumer_allocation_groups.insert(
            0,
            registers
                .iter()
                .rev()
                .map(|r| {
                    Reg::from_field(*r, Class::General)
                        .virtual_register()
                        .unwrap()
                })
                .collect(),
        );
        let ordinary = self
            .nonvolatile_pointer_bindings
            .iter()
            .any(|name| self.lookup_general(name) == Some(plan.base));
        let events = schedule(
            &plan,
            self.behavior.member_value_schedule,
            self.behavior.scheduler_enabled,
            ordinary,
        );
        (emit(plan, &registers, &events), registers)
    }

    fn retain_member_store_regions(&mut self) {
        if self.output.is_asm
            || !self.output.entry_points.is_empty()
            || !self.output.jump_tables.is_empty()
            || self
                .output
                .instructions
                .iter()
                .any(|i| matches!(i, Instruction::VerbatimWord(_)))
        {
            return;
        }
        let live = mwcc_vreg::analyze(&self.output.instructions);
        let entries: std::collections::HashSet<_> = self
            .output
            .instructions
            .iter()
            .filter_map(|i| match i {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => Some(*target),
                _ => None,
            })
            .collect();
        let mut regions = Vec::new();
        let mut start = 0;
        while start < self.output.instructions.len() {
            if !matches!(
                self.output.instructions[start],
                Instruction::AddImmediate { d: 0, .. }
            ) {
                start += 1;
                continue;
            }
            let end = member_run_end(&self.output.instructions, start, &entries);
            if let Some(plan) = region_plan(&self.output, start, end, &live) {
                let guarded = guarded_stores(&self.output, end, &plan, &live);
                regions.push((start, end, plan, guarded));
            }
            start = end;
        }
        for (start, end, plan, guarded) in regions.into_iter().rev() {
            let (mut code, registers) = self.emit_member_values(&plan);
            if let Some(guarded) = guarded {
                for index in guarded.stores {
                    match &mut self.output.instructions[index] {
                        Instruction::StoreWord { s, .. }
                        | Instruction::StoreHalfword { s, .. }
                        | Instruction::StoreByte { s, .. } => *s = registers[guarded.value],
                        _ => unreachable!("the guarded plan owns its stores"),
                    }
                }
                crate::remove_instruction_retargeting_to_next(self, guarded.load);
            }
            code.pop(); // The original region's successor owns control flow.
            self.output.instructions[start] = code[0].clone();
            for _ in start + 1..end {
                crate::remove_instruction_retargeting_to_next(self, start + 1);
            }
            for (index, instruction) in code.into_iter().enumerate().skip(1) {
                crate::insert_instruction_retargeting(self, start + index, instruction);
            }
        }
    }
}

fn store_parts(instruction: &Instruction) -> Option<(u32, u32)> {
    match *instruction {
        Instruction::StoreWord { s, a, .. }
        | Instruction::StoreHalfword { s, a, .. }
        | Instruction::StoreByte { s, a, .. } => Some((s, a)),
        _ => None,
    }
}

fn member_run_end(
    instructions: &[Instruction],
    start: usize,
    entries: &std::collections::HashSet<usize>,
) -> usize {
    let base = instructions[start + 1..]
        .iter()
        .take_while(|i| {
            matches!(i, Instruction::AddImmediate { d: 0, .. }) || store_parts(i).is_some()
        })
        .find_map(store_parts)
        .map(|(_, base)| base);
    let mut end = start + 1;
    while end < instructions.len()
        && !entries.contains(&end)
        && (matches!(instructions[end],
            Instruction::AddImmediate { d: 0, a, .. }
                if a == 0 || Some(a) == base)
            || store_parts(&instructions[end]).is_some_and(|(_, a)| Some(a) == base))
    {
        end += 1;
    }
    // A literal immediately before a different-base store belongs to
    // that next run. Do not swallow its definition with this prefix.
    if end > start + 1
        && matches!(
            instructions[end - 1],
            Instruction::AddImmediate { d: 0, .. }
        )
    {
        end -= 1;
    }
    end
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    let (last, body) = instructions.split_last()?;
    if *last != Instruction::BranchToLinkRegister {
        return None;
    }
    plan_body(body)
}

fn plan_body(body: &[Instruction]) -> Option<Plan> {
    let base = body.iter().find_map(store_parts)?.1;
    if base <= 2 {
        return None;
    }
    let mut values = Vec::new();
    let mut stores = Vec::new();
    let mut current = None;
    let mut used = true;
    for instruction in body {
        match *instruction {
            Instruction::AddImmediate { d: 0, a, immediate } if a == 0 || a == base.into() => {
                if !used {
                    return None;
                }
                let value = if a == 0 {
                    Value::Constant(immediate)
                } else {
                    Value::Address(immediate)
                };
                let index = values.iter().position(|v| *v == value).unwrap_or_else(|| {
                    values.push(value);
                    values.len() - 1
                });
                current = Some(index);
                used = false;
            }
            _ => {
                let (source, store_base) = store_parts(instruction)?;
                if store_base != base {
                    return None;
                }
                if source != 0 {
                    // Independent stores neither define the member base nor
                    // consume the scratch value. Preserve their exact position
                    // among the stores while sharing surrounding values.
                    stores.push((instruction.clone(), None));
                    continue;
                }
                let value = current?;
                // Interior pointers use complete word stores.
                if matches!(values[value], Value::Address(_))
                    && !matches!(instruction, Instruction::StoreWord { .. })
                {
                    return None;
                }
                stores.push((instruction.clone(), Some(value)));
                used = true;
            }
        }
    }
    // Keep pure constant runs with their existing owner; this measured family
    // combines an interior pointer with one or two distinct immediate values.
    if !used
        || !(2..=3).contains(&values.len())
        || values
            .iter()
            .filter(|v| matches!(v, Value::Address(_)))
            .count()
            != 1
        || stores.len() <= values.len()
    {
        return None;
    }
    Some(Plan {
        base: base.into(),
        values,
        stores,
    })
}

// Only r0 is replaced by the shared values. Independent store sources remain
// live through allocation and keep their original store order. The member base is never
// defined in a recognized run, and r0 must be dead on every outgoing edge.
// Metadata outside the region is retained by the common instruction editors.
fn region_plan(
    function: &mwcc_machine_code::MachineFunction,
    start: usize,
    end: usize,
    live: &mwcc_vreg::Liveness,
) -> Option<Plan> {
    if function.instructions.iter().any(|i| {
        matches!(i,
        Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. }
            if (start + 1..end).contains(target))
    }) || function
        .relocations
        .iter()
        .any(|r| (start..end).contains(&r.instruction_index))
        || function
            .deferred_displacements
            .iter()
            .any(|r| (start..end).contains(&r.instruction_index))
        || live.pinned.iter().any(|p| {
            p.class == Class::General
                && p.register == 0
                && p.live_slots
                    .as_ref()
                    .is_some_and(|slots| slots.binary_search(&(2 * end)).is_ok())
        })
    {
        return None;
    }
    let plan = plan_body(function.instructions.get(start..end)?)?;
    (plan.values.len() + plan.stores.len() < end - start).then_some(plan)
}

/// A fallthrough store arm dominated by the preceding member-value graph.
/// Its only scratch definition may reuse a value already present in the graph;
/// allocation then owns the extended lifetime across the condition.
#[derive(Debug)]
struct GuardedStores {
    load: usize,
    stores: std::ops::Range<usize>,
    value: usize,
}

fn guarded_stores(
    function: &mwcc_machine_code::MachineFunction,
    end: usize,
    plan: &Plan,
    live: &mwcc_vreg::Liveness,
) -> Option<GuardedStores> {
    let instructions = &function.instructions;
    if !matches!(
        instructions.get(end),
        Some(
            Instruction::CompareWordImmediate { .. }
                | Instruction::CompareLogicalWordImmediate { .. }
        )
    ) {
        return None;
    }
    let Instruction::BranchConditionalForward { target, .. } = *instructions.get(end + 1)? else {
        return None;
    };
    let load = end + 2;
    let Instruction::AddImmediate {
        d: 0,
        a: 0,
        immediate,
    } = *instructions.get(load)?
    else {
        return None;
    };
    let value = plan
        .values
        .iter()
        .position(|v| *v == Value::Constant(immediate))?;
    let mut arm_end = load + 1;
    while arm_end < target
        && instructions
            .get(arm_end)
            .and_then(store_parts)
            .is_some_and(|(s, a)| s == 0 && a > 2)
    {
        arm_end += 1;
    }
    if arm_end == load + 1
        || target > instructions.len()
        || !(arm_end == target
            || matches!(instructions.get(arm_end),
            Some(Instruction::Branch { target: join }) if *join >= target))
        || instructions.iter().any(|i| {
            matches!(i,
            Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. }
                if (end..arm_end).contains(target))
        })
        || function
            .relocations
            .iter()
            .any(|r| (end..arm_end).contains(&r.instruction_index))
        || function
            .deferred_displacements
            .iter()
            .any(|r| (end..arm_end).contains(&r.instruction_index))
        || live.pinned.iter().any(|p| {
            p.class == Class::General
                && p.register == 0
                && p.live_slots
                    .as_ref()
                    .is_some_and(|slots| slots.binary_search(&(2 * arm_end)).is_ok())
        })
    {
        return None;
    }
    Some(GuardedStores {
        load,
        stores: load + 1..arm_end,
        value,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    Value(usize),
    Store(usize),
}

fn schedule(plan: &Plan, style: MemberValueSchedule, enabled: bool, ordinary: bool) -> Vec<Event> {
    let count = plan.values.len();
    let mut order: Vec<_> = (0..count).collect();
    let uses = |value| {
        plan.stores
            .iter()
            .filter(|(_, v)| *v == Some(value))
            .count()
    };
    let address = |value| matches!(plan.values[value], Value::Address(_));
    if !enabled {
        let mut seen = vec![false; count];
        let mut events = Vec::new();
        for (store, (_, value)) in plan.stores.iter().enumerate() {
            if let Some(value) = value {
                if !seen[*value] {
                    events.push(Event::Value(*value));
                    seen[*value] = true;
                }
            }
            events.push(Event::Store(store));
        }
        return events;
    }
    let mut serial_tail = false;
    let leading = match style {
        MemberValueSchedule::FirstStore => 1,
        MemberValueSchedule::TwoValues => 2,
        MemberValueSchedule::AddressEarly => {
            order[1..].sort_by_key(|v| !address(*v));
            if address(0) {
                1
            } else {
                2
            }
        }
        MemberValueSchedule::ReadyValues {
            ordered_issue_width,
        } => {
            if ordinary {
                // Shared values are ready roots; an interior address outranks
                // the remaining single-use literal. Ties retain source order.
                order.sort_by_key(|v| {
                    if uses(*v) > 1 {
                        0
                    } else if address(*v) {
                        1
                    } else {
                        2
                    }
                });
                count
            } else {
                // An ordered store keeps its first source value at the head.
                // Reused leading values permit the address to start early;
                // otherwise subsequent values issue with successive stores.
                if uses(0) > 1 {
                    order[1..].sort_by_key(|v| !address(*v));
                } else {
                    serial_tail = true;
                }
                usize::from(ordered_issue_width)
            }
        }
    }
    .clamp(1, count);
    let mut events: Vec<_> = order[..leading].iter().map(|v| Event::Value(*v)).collect();
    events.push(Event::Store(0));
    let mut next_store = 1;
    for value in &order[leading..] {
        events.push(Event::Value(*value));
        if serial_tail {
            events.push(Event::Store(next_store));
            next_store += 1;
        }
    }
    for store in next_store..plan.stores.len() {
        events.push(Event::Store(store));
    }
    events
}

fn emit(plan: &Plan, registers: &[u32], events: &[Event]) -> Vec<Instruction> {
    let mut instructions = Vec::new();
    let load = |index: usize| match plan.values[index] {
        Value::Constant(immediate) => Instruction::AddImmediate {
            d: registers[index],
            a: 0,
            immediate,
        },
        Value::Address(immediate) => Instruction::AddImmediate {
            d: registers[index],
            a: plan.base,
            immediate,
        },
    };
    let store = |index: usize| {
        let (mut instruction, value) = plan.stores[index].clone();
        let Some(value) = value else {
            return instruction;
        };
        match &mut instruction {
            Instruction::StoreWord { s, .. }
            | Instruction::StoreHalfword { s, .. }
            | Instruction::StoreByte { s, .. } => *s = registers[value],
            _ => unreachable!("classified store"),
        }
        instruction
    };
    for event in events {
        instructions.push(match *event {
            Event::Value(value) => load(value),
            Event::Store(index) => store(index),
        });
    }
    instructions.push(Instruction::BranchToLinkRegister);
    instructions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> Vec<Instruction> {
        vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: 2,
            },
            Instruction::load_immediate(0, 164),
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 20,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 16,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: 28,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: 20,
            },
            Instruction::BranchToLinkRegister,
        ]
    }

    #[test]
    fn every_schedule_defines_values_before_preserving_each_source_store() {
        let styles = [
            MemberValueSchedule::FirstStore,
            MemberValueSchedule::TwoValues,
            MemberValueSchedule::AddressEarly,
            MemberValueSchedule::ReadyValues {
                ordered_issue_width: 2,
            },
            MemberValueSchedule::ReadyValues {
                ordered_issue_width: 1,
            },
        ];
        // Enumerate first-use-ordered graphs, including adjacent repeated
        // stores, late shared values, and interior pointers at every position.
        for encoded in 0usize..3usize.pow(5) {
            let values: Vec<_> = (0..5).map(|at| (encoded / 3usize.pow(at)) % 3).collect();
            let mut seen = Vec::new();
            for value in &values {
                if !seen.contains(value) {
                    seen.push(*value);
                }
            }
            if seen != [0, 1, 2] {
                continue;
            }
            for address in 0..3 {
                let plan = Plan {
                    base: 4,
                    values: (0..3)
                        .map(|v| {
                            if v == address {
                                Value::Address(20)
                            } else {
                                Value::Constant(v as i16)
                            }
                        })
                        .collect(),
                    stores: values
                        .iter()
                        .enumerate()
                        .map(|(at, v)| {
                            (
                                Instruction::StoreWord {
                                    s: 0,
                                    a: 4,
                                    offset: 4 * at as i16,
                                },
                                Some(*v),
                            )
                        })
                        .collect(),
                };
                for style in styles {
                    for enabled in [false, true] {
                        for ordinary in [false, true] {
                            let events = schedule(&plan, style, enabled, ordinary);
                            let mut defined = [false; 3];
                            let mut next_store = 0;
                            for event in events {
                                match event {
                                    Event::Value(v) => {
                                        assert!(!defined[v]);
                                        defined[v] = true;
                                    }
                                    Event::Store(at) => {
                                        assert_eq!(at, next_store);
                                        assert!(defined[values[at]], "{style:?}: {values:?}");
                                        next_store += 1;
                                    }
                                }
                            }
                            assert_eq!(next_store, values.len());
                            assert!(defined.into_iter().all(|v| v));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn shares_repeated_values_without_changing_store_order() {
        let original = input();
        let plan = plan(&original).unwrap();
        assert_eq!(
            plan.values,
            [Value::Constant(0), Value::Constant(164), Value::Address(20)]
        );
        let events = schedule(&plan, MemberValueSchedule::FirstStore, true, true);
        let result = emit(&plan, &[35, 34, 33], &events);
        assert_eq!(result.len(), 10);
        assert!(matches!(
            result[0],
            Instruction::AddImmediate {
                d: 35,
                a: 0,
                immediate: 0
            }
        ));
        assert!(matches!(
            result[2],
            Instruction::AddImmediate {
                d: 34,
                a: 0,
                immediate: 164
            }
        ));
        assert!(matches!(
            result[3],
            Instruction::AddImmediate {
                d: 33,
                a: 4,
                immediate: 20
            }
        ));
        let offsets = |instructions: &[Instruction]| {
            instructions
                .iter()
                .filter_map(|i| match i {
                    Instruction::StoreWord { offset, .. }
                    | Instruction::StoreHalfword { offset, .. } => Some(*offset),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(offsets(&original), offsets(&result));
    }

    #[test]
    fn shares_a_prefix_address_across_independent_member_stores() {
        let mut instructions = vec![
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 20,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 5,
                a: 4,
                offset: 12,
            },
        ];
        instructions.extend(input());
        let plan = plan(&instructions).unwrap();
        assert_eq!(
            plan.values
                .iter()
                .filter(|v| **v == Value::Address(20))
                .count(),
            1
        );
        for enabled in [false, true] {
            let events = schedule(&plan, MemberValueSchedule::FirstStore, enabled, true);
            let result = emit(&plan, &[35, 34, 33], &events);
            let stores: Vec<_> = result.iter().filter(|i| store_parts(i).is_some()).collect();
            assert_eq!(stores.len(), 8);
            assert_eq!(
                *stores[1],
                Instruction::StoreWord {
                    s: 5,
                    a: 4,
                    offset: 12
                }
            );
            // Both writes remain observable and share the single interior value.
            assert_eq!(store_parts(stores[0]), Some((35, 4)));
            assert_eq!(store_parts(stores[5]), Some((35, 4)));
            assert_eq!(
                result
                    .iter()
                    .filter(|i| matches!(
                        i,
                        Instruction::AddImmediate {
                            a: 4,
                            immediate: 20,
                            ..
                        }
                    ))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn a_new_member_base_keeps_its_leading_literal() {
        let mut instructions = vec![
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 20,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 5,
                a: 3,
                offset: 12,
            },
        ];
        instructions.extend(input());
        let entries = std::collections::HashSet::new();
        assert_eq!(member_run_end(&instructions, 0, &entries), 3);
        let end = member_run_end(&instructions, 3, &entries);
        assert_eq!(end, instructions.len() - 1);
        assert!(plan_body(&instructions[3..end]).is_some());
    }

    fn guarded_input() -> (mwcc_machine_code::MachineFunction, usize, Plan) {
        let mut instructions = input();
        instructions.pop();
        let end = instructions.len();
        let plan = plan_body(&instructions).unwrap();
        instructions.extend([
            Instruction::CompareWordImmediate { a: 6, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: end + 5,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 0,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: 2,
            },
            Instruction::BranchToLinkRegister,
        ]);
        (
            mwcc_machine_code::MachineFunction {
                instructions,
                ..Default::default()
            },
            end,
            plan,
        )
    }

    #[test]
    fn extends_a_member_constant_into_a_dominated_store_arm() {
        let (function, end, plan) = guarded_input();
        let reuse = guarded_stores(
            &function,
            end,
            &plan,
            &mwcc_vreg::analyze(&function.instructions),
        )
        .unwrap();
        assert_eq!(reuse.load, end + 2);
        assert_eq!(reuse.stores, end + 3..end + 5);
        assert_eq!(plan.values[reuse.value], Value::Constant(0));
    }

    #[test]
    fn guarded_reuse_rejects_bypassed_definitions_and_live_scratch() {
        for at in 0..5 {
            let (mut function, end, plan) = guarded_input();
            function
                .instructions
                .push(Instruction::Branch { target: end + at });
            assert!(guarded_stores(
                &function,
                end,
                &plan,
                &mwcc_vreg::analyze(&function.instructions)
            )
            .is_none());
        }
        let (mut function, end, plan) = guarded_input();
        function
            .instructions
            .insert(end + 5, Instruction::move_register(3, 0));
        assert!(guarded_stores(
            &function,
            end,
            &plan,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_none());
    }

    #[test]
    fn guarded_reuse_rejects_new_values_calls_and_symbolic_fixups() {
        for replacement in [
            Instruction::load_immediate(0, 7),
            Instruction::BranchAndLink {
                target: "observe".into(),
            },
        ] {
            let (mut function, end, plan) = guarded_input();
            function.instructions[end + 2] = replacement;
            assert!(guarded_stores(
                &function,
                end,
                &plan,
                &mwcc_vreg::analyze(&function.instructions)
            )
            .is_none());
        }
        let (mut function, end, plan) = guarded_input();
        function
            .deferred_displacements
            .push(mwcc_machine_code::DeferredDisplacement {
                instruction_index: end + 3,
                target: mwcc_machine_code::DeferredDisplacementTarget::Symbol("global".into()),
            });
        assert!(guarded_stores(
            &function,
            end,
            &plan,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_none());
    }

    #[test]
    fn rejects_control_flow_reads_other_bases_and_live_scratch_return() {
        for extra in [
            Instruction::Branch { target: 0 },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 0,
            },
            Instruction::Or { a: 3, s: 0, b: 0 },
        ] {
            let mut instructions = input();
            instructions.insert(instructions.len() - 1, extra);
            assert!(plan(&instructions).is_none());
        }
    }
    #[test]
    fn recognizes_an_interior_run_but_preserves_live_scratch() {
        let mut function = mwcc_machine_code::MachineFunction::new("initialize");
        function.instructions.push(Instruction::BranchAndLink {
            target: "before".into(),
        });
        function.instructions.extend(input());
        let end = function.instructions.len() - 1;
        assert!(region_plan(
            &function,
            1,
            end,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_some());
        function
            .instructions
            .insert(end, Instruction::move_register(3, 0));
        assert!(region_plan(
            &function,
            1,
            end,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_none());
    }

    #[test]
    fn rejects_interior_entries_and_symbolic_value_ownership() {
        let mut function = mwcc_machine_code::MachineFunction::new("initialize");
        function.instructions = input();
        let end = function.instructions.len() - 1;
        function
            .instructions
            .push(Instruction::Branch { target: 1 });
        assert!(region_plan(
            &function,
            0,
            end,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_none());
        function.instructions.pop();
        function
            .deferred_displacements
            .push(mwcc_machine_code::DeferredDisplacement {
                instruction_index: 0,
                target: mwcc_machine_code::DeferredDisplacementTarget::Symbol("global".into()),
            });
        assert!(region_plan(
            &function,
            0,
            end,
            &mwcc_vreg::analyze(&function.instructions)
        )
        .is_none());
    }
}
