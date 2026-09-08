//! Share register-derived address halves across halfword publications.
//!
//! A local value graph follows copies, constant address offsets and high-half
//! extraction. Stores retain their original targets, so aliasing does not
//! invalidate these register values. Version policy controls the one measured
//! ordinary leaf store exchange; volatile stores preserve source order.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction as I, MachineFunction};
use mwcc_syntax_trees::{BinaryOperator as B, Expression as E, Function, Statement, Type};
use mwcc_versions::Optimization;
use mwcc_vreg::{Class, Reg};
use std::collections::{HashMap, HashSet};

/// The legacy indexed-store gate may defer unrelated scheduling families, but
/// casts, constant address additions and high-half extraction from parameters
/// already have ordinary scalar lowering. They do not read through stores.
pub(crate) fn supports_source_run(function: &Function) -> bool {
    fn value(e: &E, function: &Function) -> bool {
        match e {
            E::Variable(name) => function.parameters.iter().any(|p| {
                p.name == *name
                    && matches!(
                        p.parameter_type,
                        Type::Int
                            | Type::UnsignedInt
                            | Type::Pointer(_)
                            | Type::StructPointer { .. }
                    )
            }),
            E::Cast {
                target_type:
                    Type::Int | Type::UnsignedInt | Type::UnsignedShort | Type::Short | Type::Pointer(_),
                operand,
            } => value(operand, function),
            E::Binary {
                operator: B::Add | B::Subtract,
                left,
                right,
            } => {
                value(left, function)
                    && matches!(right.as_ref(), E::IntegerLiteral(n) if i16::try_from(*n).is_ok())
            }
            E::Binary {
                operator: B::ShiftRight,
                left,
                right,
            } => value(left, function) && matches!(right.as_ref(), E::IntegerLiteral(16)),
            _ => false,
        }
    }
    fn target(e: &E, function: &Function) -> bool {
        let pointer = match e {
            E::Index { base, index } if matches!(index.as_ref(),E::IntegerLiteral(n) if (0..=16383).contains(n)) => {
                base.as_ref()
            }
            E::Dereference { pointer } => pointer.as_ref(),
            _ => return false,
        };
        let E::Variable(name) = pointer else {
            return false;
        };
        function.parameters.iter().any(|p| {
            p.name == *name
                && matches!(
                    p.parameter_type,
                    Type::Pointer(
                        mwcc_syntax_trees::Pointee::Short
                            | mwcc_syntax_trees::Pointee::UnsignedShort
                    )
                )
        })
    }
    function.statements.iter().all(|statement| match statement {
        Statement::Store {
            target: location,
            value: stored,
        } => target(location, function) && value(stored, function),
        _ => false,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Address {
    base: u8,
    offset: i16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Value {
    Low(Address),
    High(Address),
}
impl Value {
    fn address(self) -> Address {
        match self {
            Self::Low(a) | Self::High(a) => a,
        }
    }
}
struct Store {
    base: u8,
    offset: i16,
    value: Value,
}
struct Plan {
    start: usize,
    end: usize,
    stores: Vec<Store>,
}

impl Generator {
    pub(crate) fn retain_split_address_stores(&mut self, return_type: Type) {
        if self.behavior.optimization < Optimization::O2
            || self.output.is_asm
            || !self.output.jump_tables.is_empty()
            || !self.output.entry_points.is_empty()
            || self
                .output
                .instructions
                .iter()
                .any(|i| matches!(i, I::VerbatimWord(_)))
        {
            return;
        }
        let entries: HashSet<_> = self
            .output
            .instructions
            .iter()
            .filter_map(|i| match i {
                I::Branch { target } | I::BranchConditionalForward { target, .. } => Some(*target),
                _ => None,
            })
            .collect();
        let live = mwcc_vreg::analyze(&self.output.instructions);
        let mut plans = Vec::new();
        let mut start = 0;
        while start < self.output.instructions.len() {
            let mut end = start;
            while end < self.output.instructions.len()
                && (end == start || !entries.contains(&end))
                && eligible(&self.output.instructions[end])
            {
                end += 1;
            }
            if let Some(plan) = recognize(&self.output, start, end, &live, return_type) {
                plans.push(plan);
            }
            start = if end == start { start + 1 } else { end };
        }
        for mut plan in plans.into_iter().rev() {
            let mut leading_high = None;
            let schedule =
                self.behavior.scheduler_enabled && self.behavior.optimization >= Optimization::O3;
            if schedule
                && self.behavior.split_address_low_store_first
                && plan.start == 0
                && plan.end + 1 == self.output.instructions.len()
                && matches!(self.output.instructions[plan.end], I::BranchToLinkRegister)
                && self
                    .nonvolatile_pointer_bindings
                    .iter()
                    .any(|name| self.lookup_general(name) == Some(plan.stores[0].base))
            {
                let first = &plan.stores[0];
                let second = &plan.stores[1];
                if matches!((first.value,second.value),(Value::High(a),Value::Low(b)) if a==b)
                    && first.base == second.base
                    && (i32::from(first.offset) - i32::from(second.offset)).abs() >= 2
                {
                    leading_high = Some(first.value);
                    plan.stores.swap(0, 1);
                }
            }
            let mut homes = HashMap::new();
            let mut code = Vec::new();
            if let Some(value) = leading_high {
                materialize(self, value, &mut homes, &mut code);
            }
            for (index, store) in plan.stores.iter().enumerate() {
                let source = materialize(self, store.value, &mut homes, &mut code);
                code.push(I::StoreHalfword {
                    s: source,
                    a: store.base,
                    offset: store.offset,
                });
                // Measured scalar issue: first store, then the other independent
                // address halves, then their stores. With scheduling disabled,
                // retain each value at its first use instead.
                if index == 0 && schedule {
                    for later in &plan.stores[1..] {
                        materialize(self, later.value, &mut homes, &mut code);
                    }
                }
            }
            if code.len() >= plan.end - plan.start {
                continue;
            }
            self.output.instructions[plan.start] = code[0].clone();
            for _ in plan.start + 1..plan.end {
                crate::remove_instruction_retargeting_to_next(self, plan.start + 1);
            }
            for (index, instruction) in code.into_iter().enumerate().skip(1) {
                crate::insert_instruction_retargeting(self, plan.start + index, instruction);
            }
        }
    }
}

fn materialize(
    g: &mut Generator,
    value: Value,
    homes: &mut HashMap<Value, u8>,
    code: &mut Vec<I>,
) -> u8 {
    if let Some(&r) = homes.get(&value) {
        return r;
    }
    let address = value.address();
    let r = match value {
        Value::Low(Address { base, offset: 0 }) => base,
        Value::Low(Address { base, offset }) => {
            let r = g.fresh_virtual_general_preferring(0);
            code.push(I::AddImmediate {
                d: r,
                a: base,
                immediate: offset,
            });
            r
        }
        Value::High(_) => {
            let source = materialize(g, Value::Low(address), homes, code);
            let r = g.fresh_virtual_general_preferring(0);
            code.push(I::ShiftRightLogicalImmediate {
                a: r,
                s: source,
                shift: 16,
            });
            r
        }
    };
    homes.insert(value, r);
    r
}
fn eligible(i: &I) -> bool {
    if matches!(i, I::Or {s,b,..} if s==b) {
        return true;
    }
    matches!(
        i,
        I::AddImmediate { a: 1..=u8::MAX, .. }
            | I::ShiftRightLogicalImmediate { shift: 16, .. }
            | I::StoreHalfword { .. }
    )
}
fn recognize(
    function: &MachineFunction,
    start: usize,
    end: usize,
    live: &mwcc_vreg::Liveness,
    return_type: Type,
) -> Option<Plan> {
    if end <= start
        || function
            .relocations
            .iter()
            .any(|r| (start..end).contains(&r.instruction_index))
        || function
            .deferred_displacements
            .iter()
            .any(|d| (start..end).contains(&d.instruction_index))
    {
        return None;
    }
    let mut values = HashMap::new();
    let mut definitions = HashSet::new();
    let mut roots = HashSet::new();
    let mut stores = Vec::new();
    let mut high_stores = 0;
    fn read(values: &HashMap<u8, Value>, register: u8) -> Value {
        values
            .get(&register)
            .copied()
            .unwrap_or(Value::Low(Address {
                base: register,
                offset: 0,
            }))
    }
    for i in &function.instructions[start..end] {
        match *i {
            I::Or { a, s, b } if s == b => {
                let value = read(&values, s);
                roots.insert(value.address().base);
                definitions.insert(a);
                values.insert(a, value);
            }
            I::AddImmediate { d, a, immediate } => {
                let Value::Low(mut address) = read(&values, a) else {
                    return None;
                };
                address.offset = address.offset.checked_add(immediate)?;
                roots.insert(address.base);
                definitions.insert(d);
                values.insert(d, Value::Low(address));
            }
            I::ShiftRightLogicalImmediate { a, s, shift: 16 } => {
                let Value::Low(address) = read(&values, s) else {
                    return None;
                };
                roots.insert(address.base);
                definitions.insert(a);
                values.insert(a, Value::High(address));
            }
            I::StoreHalfword { s, a, offset } => {
                let value = read(&values, s);
                roots.insert(value.address().base);
                roots.insert(a);
                high_stores += usize::from(matches!(value, Value::High(_)));
                stores.push(Store {
                    base: a,
                    offset,
                    value,
                });
            }
            _ => return None,
        }
    }
    if stores.len() < 4
        || high_stores < 2
        || !definitions.is_disjoint(&roots)
        || (return_type != Type::Void && definitions.contains(&3))
        || definitions.iter().any(|r| live_at(live, *r, end))
    {
        return None;
    }
    Some(Plan { start, end, stores })
}
fn live_at(live: &mwcc_vreg::Liveness, register: u8, at: usize) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{DeferredDisplacement, DeferredDisplacementTarget};
    fn packet(offset: i16) -> MachineFunction {
        let mut f = MachineFunction::new("split");
        for base in [3, 4] {
            f.instructions.extend([
                I::AddImmediate {
                    d: 0,
                    a: 5,
                    immediate: offset,
                },
                I::ShiftRightLogicalImmediate {
                    a: 0,
                    s: 0,
                    shift: 16,
                },
                I::StoreHalfword {
                    s: 0,
                    a: base,
                    offset: 0,
                },
                I::AddImmediate {
                    d: 0,
                    a: 5,
                    immediate: offset,
                },
                I::StoreHalfword {
                    s: 0,
                    a: base,
                    offset: 2,
                },
            ]);
        }
        f.instructions.push(I::BranchToLinkRegister);
        f
    }
    fn plan(f: &MachineFunction, end: usize, ty: Type) -> Option<Plan> {
        recognize(f, 0, end, &mwcc_vreg::analyze(&f.instructions), ty)
    }
    #[test]
    fn resolves_copy_chains_and_preserves_aliasing_store_order() {
        let mut f = packet(244);
        f.instructions[0] = I::move_register(0, 5);
        f.instructions.insert(
            1,
            I::AddImmediate {
                d: 0,
                a: 5,
                immediate: 244,
            },
        );
        let p = plan(&f, 11, Type::Void).unwrap();
        assert_eq!(
            p.stores
                .iter()
                .map(|s| (s.base, s.offset))
                .collect::<Vec<_>>(),
            [(3, 0), (3, 2), (4, 0), (4, 2)]
        );
        assert_eq!(p.stores[0].value, p.stores[2].value);
        assert_eq!(
            p.stores[1].value,
            Value::Low(Address {
                base: 5,
                offset: 244
            })
        );
    }
    #[test]
    fn keeps_negative_offsets_and_rejects_overflowing_combination() {
        let f = packet(-20);
        assert_eq!(
            plan(&f, 10, Type::Void).unwrap().stores[0].value,
            Value::High(Address {
                base: 5,
                offset: -20
            })
        );
        let mut f = packet(32767);
        f.instructions.insert(
            1,
            I::AddImmediate {
                d: 0,
                a: 0,
                immediate: 1,
            },
        );
        assert!(plan(&f, 11, Type::Void).is_none());
    }
    #[test]
    fn rejects_mutated_bases_and_live_scratch_outputs() {
        let mut f = packet(0);
        f.instructions.insert(10, I::move_register(3, 0));
        assert!(plan(&f, 10, Type::UnsignedInt).is_none());
        f = packet(0);
        f.instructions.insert(
            0,
            I::AddImmediate {
                d: 5,
                a: 5,
                immediate: 4,
            },
        );
        assert!(plan(&f, 11, Type::Void).is_none());
    }
    #[test]
    fn return_home_is_observable_even_without_an_explicit_return_operand() {
        let mut f = packet(0);
        for i in &mut f.instructions {
            if let I::StoreHalfword { a, .. } = i {
                if *a == 3 {
                    *a = 6;
                }
            }
        }
        f.instructions.insert(10, I::move_register(3, 0));
        assert!(plan(&f, 11, Type::UnsignedInt).is_none());
    }
    #[test]
    fn symbolic_displacements_stay_with_their_existing_owner() {
        let mut f = packet(0);
        f.deferred_displacements.push(DeferredDisplacement {
            instruction_index: 0,
            target: DeferredDisplacementTarget::SymbolAddress("array".into()),
        });
        assert!(plan(&f, 10, Type::Void).is_none());
    }
}
