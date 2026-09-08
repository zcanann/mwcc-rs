//! Retain values across a leaf member-initialization run before allocation.
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
    base: u8,
    values: Vec<Value>,
    stores: Vec<(Instruction, usize)>,
}

impl Generator {
    pub(crate) fn retain_member_store_values(&mut self) {
        if !matches!(
            self.behavior.optimization,
            Optimization::O2 | Optimization::O3 | Optimization::O4
        ) || !self.output.relocations.is_empty()
            || !self.output.entry_points.is_empty()
            || !self.output.jump_tables.is_empty()
            || !self.output.deferred_displacements.is_empty()
        {
            return;
        }
        let Some(plan) = plan(&self.output.instructions) else {
            return;
        };
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
        self.output.instructions = emit(&plan, &registers, &events);
    }
}

fn store_parts(instruction: &Instruction) -> Option<(u8, u8)> {
    match *instruction {
        Instruction::StoreWord { s, a, .. }
        | Instruction::StoreHalfword { s, a, .. }
        | Instruction::StoreByte { s, a, .. } => Some((s, a)),
        _ => None,
    }
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    let (last, body) = instructions.split_last()?;
    if *last != Instruction::BranchToLinkRegister {
        return None;
    }
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
            Instruction::AddImmediate { d: 0, a, immediate } if a == 0 || a == base => {
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
                if store_parts(instruction) != Some((0, base)) {
                    return None;
                }
                let value = current?;
                // Interior pointers use complete word stores.
                if matches!(values[value], Value::Address(_))
                    && !matches!(instruction, Instruction::StoreWord { .. })
                {
                    return None;
                }
                stores.push((instruction.clone(), value));
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
        base,
        values,
        stores,
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
    let uses = |value| plan.stores.iter().filter(|(_, v)| *v == value).count();
    let address = |value| matches!(plan.values[value], Value::Address(_));
    if !enabled {
        let mut seen = vec![false; count];
        let mut events = Vec::new();
        for (store, (_, value)) in plan.stores.iter().enumerate() {
            if !seen[*value] {
                events.push(Event::Value(*value));
                seen[*value] = true;
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

fn emit(plan: &Plan, registers: &[u8], events: &[Event]) -> Vec<Instruction> {
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
                                *v,
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
}
