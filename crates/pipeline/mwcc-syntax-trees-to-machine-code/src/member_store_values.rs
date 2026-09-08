//! Retain values across a leaf member-initialization run before allocation.
//!
//! Constants and interior pointers share the same store-value graph. The old
//! scheduler issues its first store immediately, then fills two value lanes
//! before each pending store. Stores retain their original order, including
//! volatile writes and assignment chains. Physical homes come from liveness.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_versions::{ConstantStoreScheduleStyle, Optimization};
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
        if self.behavior.constant_store_schedule_style
            != ConstantStoreScheduleStyle::InterleavedPairs
            || !self.behavior.scheduler_enabled
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
            || !self.output.relocations.is_empty()
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
        self.output.instructions = emit(&plan, &registers);
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

fn emit(plan: &Plan, registers: &[u8]) -> Vec<Instruction> {
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
    instructions.extend([load(0), store(0)]);
    for index in 1..plan.values.len() {
        instructions.push(load(index));
    }
    for index in 1..plan.stores.len() {
        instructions.push(store(index));
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
    fn shares_repeated_values_without_changing_store_order() {
        let original = input();
        let plan = plan(&original).unwrap();
        assert_eq!(
            plan.values,
            [Value::Constant(0), Value::Constant(164), Value::Address(20)]
        );
        let result = emit(&plan, &[35, 34, 33]);
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
