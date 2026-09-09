//! Resolve constant address expressions at the memory use, without moving reads
//! or stores. Facts name the original base value and are invalidated whenever a
//! fixed home they depend on is overwritten. Control-flow joins discard facts.
use super::*;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum AddressBase {
    Value(Value),
    Frame,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MemoryAddress {
    pub base: AddressBase,
    pub offset: i16,
}

impl MemoryAddress {
    pub(super) fn new(pointer: Value) -> Self {
        Self {
            base: AddressBase::Value(pointer),
            offset: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct Resolved {
    base: AddressBase,
    displacement: u32,
}

fn id(value: Value) -> Option<usize> {
    match value.source {
        Source::Register(id) => Some(id),
        _ => None,
    }
}

fn pointer(value: Value) -> bool {
    matches!(value.ty, Type::Pointer(_) | Type::StructPointer { .. })
}

fn resolve(value: Value, facts: &HashMap<usize, Resolved>) -> Resolved {
    id(value)
        .and_then(|id| facts.get(&id).copied())
        .unwrap_or(Resolved {
            base: AddressBase::Value(value),
            displacement: 0,
        })
}

fn written(op: &Operation) -> Option<Value> {
    match op {
        Operation::Local { result }
        | Operation::Parameter { result, .. }
        | Operation::Copy { result, .. }
        | Operation::Convert { result, .. }
        | Operation::Offset { result, .. }
        | Operation::FrameAddress { result, .. }
        | Operation::Address { result, .. }
        | Operation::Load { result, .. }
        | Operation::Binary { result, .. } => Some(*result),
        Operation::Call { result, .. } => *result,
        _ => None,
    }
}

fn definition(op: &Operation, facts: &HashMap<usize, Resolved>) -> Option<Resolved> {
    let (value, bytes) = match op {
        Operation::FrameAddress { offset, .. } => {
            return Some(Resolved {
                base: AddressBase::Frame,
                displacement: *offset as i32 as u32,
            })
        }
        Operation::Offset { pointer, bytes, .. } => (*pointer, *bytes),
        Operation::Copy { result, value } | Operation::Convert { result, value }
            if pointer(*result) && pointer(*value) =>
        {
            (*value, 0)
        }
        Operation::Binary {
            result,
            operator,
            left,
            right,
            ..
        } if pointer(*result) => match (operator, left.source, right.source) {
            (BinaryOperator::Add, _, Source::Constant(n)) if pointer(*left) => (*left, n as u32),
            (BinaryOperator::Add, Source::Constant(n), _) if pointer(*right) => (*right, n as u32),
            (BinaryOperator::Subtract, _, Source::Constant(n)) if pointer(*left) => {
                (*left, (n as u32).wrapping_neg())
            }
            _ => return None,
        },
        _ => return None,
    };
    let mut resolved = resolve(value, facts);
    resolved.displacement = resolved.displacement.wrapping_add(bytes);
    Some(resolved)
}

fn fold(address: &mut MemoryAddress, pair: bool, facts: &HashMap<usize, Resolved>) {
    let AddressBase::Value(value) = address.base else {
        return;
    };
    let resolved = resolve(value, facts);
    let displacement = resolved
        .displacement
        .wrapping_add(address.offset as i32 as u32) as i32;
    // Both words need an encodable displacement. In particular 32764 cannot
    // serve as the high word of a pair without retaining address arithmetic.
    if let Ok(offset) = i16::try_from(displacement) {
        if !pair || i16::try_from(displacement + 4).is_ok() {
            *address = MemoryAddress {
                base: resolved.base,
                offset,
            };
        }
    }
}

fn rewrite(
    operations: &mut [Operation],
    facts: &mut HashMap<usize, Resolved>,
    removable: &mut HashSet<usize>,
) {
    for op in operations {
        if let Operation::Branch {
            then_body,
            else_body,
            ..
        } = op
        {
            rewrite(then_body, &mut facts.clone(), removable);
            rewrite(else_body, &mut facts.clone(), removable);
            facts.clear();
            continue;
        }
        match op {
            Operation::Label(_) | Operation::Jump(_) | Operation::Return(_) => facts.clear(),
            Operation::Load { address, result } => fold(address, wide(result.ty), facts),
            Operation::Store { address, value } => fold(address, wide(value.ty), facts),
            _ => {}
        }
        let new = definition(op, facts);
        if let Some(written) = written(op).and_then(id) {
            facts.retain(|key, fact| {
                *key != written
                    && !matches!(fact.base, AddressBase::Value(value) if id(value)==Some(written))
            });
            if let Some(new) = new {
                if !matches!(new.base, AddressBase::Value(value) if id(value)==Some(written)) {
                    facts.insert(written, new);
                }
                if matches!(
                    op,
                    Operation::Offset { .. }
                        | Operation::FrameAddress { .. }
                        | Operation::Binary { .. }
                ) {
                    removable.insert(written);
                }
            }
        }
    }
}

fn address_use(address: MemoryAddress, used: &mut HashSet<usize>) {
    if let AddressBase::Value(value) = address.base {
        value_use(value, used);
    }
}

fn value_use(value: Value, used: &mut HashSet<usize>) {
    if let Some(id) = id(value) {
        used.insert(id);
    }
}

fn condition_use(condition: Condition, used: &mut HashSet<usize>) {
    match condition {
        Condition::Value(value) => value_use(value, used),
        Condition::Compare { left, right, .. } => {
            value_use(left, used);
            value_use(right, used);
        }
    }
}

fn uses(operations: &[Operation], used: &mut HashSet<usize>) {
    for op in operations {
        match op {
            Operation::Branch {
                condition,
                then_body,
                else_body,
            } => {
                condition_use(*condition, used);
                uses(then_body, used);
                uses(else_body, used);
            }
            Operation::BranchIf { condition, .. } => condition_use(*condition, used),
            Operation::Return(Some(value))
            | Operation::Copy { value, .. }
            | Operation::Convert { value, .. } => value_use(*value, used),
            Operation::Offset { pointer, .. } => value_use(*pointer, used),
            Operation::Load { address, .. } => address_use(*address, used),
            Operation::Store { address, value } => {
                address_use(*address, used);
                value_use(*value, used);
            }
            Operation::Binary { left, right, .. } => {
                value_use(*left, used);
                value_use(*right, used);
            }
            Operation::Call {
                target, arguments, ..
            } => {
                if let CallTarget::Indirect(value) = target {
                    value_use(*value, used);
                }
                for (_, value) in arguments {
                    value_use(*value, used);
                }
            }
            _ => {}
        }
    }
}

fn prune(
    operations: &mut Vec<Operation>,
    removable: &HashSet<usize>,
    used: &HashSet<usize>,
) -> usize {
    let before = operations.len();
    operations.retain(|op| {
        !written(op)
            .and_then(id)
            .is_some_and(|id| removable.contains(&id) && !used.contains(&id))
    });
    let mut removed = before - operations.len();
    for op in operations {
        if let Operation::Branch {
            then_body,
            else_body,
            ..
        } = op
        {
            removed += prune(then_body, removable, used) + prune(else_body, removable, used);
        }
    }
    removed
}

impl Graph<'_> {
    pub(super) fn fold_memory_addresses(&mut self) {
        let mut removable = HashSet::new();
        rewrite(&mut self.operations, &mut HashMap::new(), &mut removable);
        loop {
            let mut used = HashSet::new();
            uses(&self.operations, &mut used);
            if let Some(value) = self.returned {
                value_use(value, &mut used);
            }
            if prune(&mut self.operations, &removable, &used) == 0 {
                break;
            }
        }
    }
}

impl Generator {
    pub(super) fn wide_graph_memory_address(
        &mut self,
        address: MemoryAddress,
        registers: &[Option<Registers>],
    ) -> (u32, i16) {
        let base = match address.base {
            AddressBase::Frame => 1,
            AddressBase::Value(value) => self.wide_graph_operand(value, registers).low,
        };
        (base, address.offset)
    }
}
