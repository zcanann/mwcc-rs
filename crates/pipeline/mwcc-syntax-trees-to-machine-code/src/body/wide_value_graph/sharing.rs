//! GC/1.3's cross-expression promotion sharing. The first arithmetic use can
//! lose its sign word while later conversions reuse a materialized full pair.
//! A first conversion must dominate the others and execute outside a cycle.
use super::*;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Sites {
    pub expression: usize,
    definitions: HashMap<usize, (usize, bool)>,
}
impl Sites {
    pub fn record(&mut self, id: usize) {
        self.definitions.insert(id, (self.expression, false));
    }
    pub fn explicit_word_cast(&self, id: usize) -> bool {
        self.definitions.get(&id).is_some_and(|site| site.1)
    }
}
#[derive(Default)]
pub(super) struct Sharing {
    pub zero: HashSet<usize>,
    pub full: HashSet<usize>,
}

pub(super) fn word_cast(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Cast {
            target_type: Type::Int
                | Type::UnsignedInt
                | Type::Short
                | Type::UnsignedShort
                | Type::Char
                | Type::UnsignedChar,
            ..
        }
    )
}

struct Node<'a> {
    operation: &'a Operation,
    next: Vec<usize>,
}
struct Flow<'a> {
    nodes: Vec<Node<'a>>,
    labels: HashMap<usize, usize>,
}
impl<'a> Flow<'a> {
    fn new(operations: &'a [Operation]) -> Option<(Self, usize)> {
        let mut flow = Self {
            nodes: Vec::new(),
            labels: HashMap::new(),
        };
        let entry = flow.append(operations, None)?;
        for node in &mut flow.nodes {
            if let Operation::Jump(target) | Operation::BranchIf { target, .. } = node.operation {
                node.next.push(*flow.labels.get(target)?);
            }
        }
        Some((flow, entry))
    }

    fn append(&mut self, operations: &'a [Operation], mut next: Option<usize>) -> Option<usize> {
        for operation in operations.iter().rev() {
            let following = match operation {
                Operation::Branch {
                    then_body,
                    else_body,
                    ..
                } => [self.append(then_body, next), self.append(else_body, next)]
                    .into_iter()
                    .flatten()
                    .collect(),
                Operation::Jump(_) | Operation::Return(_) => Vec::new(),
                _ => next.into_iter().collect(),
            };
            let index = self.nodes.len();
            if let Operation::Label(label) = operation {
                self.labels.insert(*label, index);
            }
            self.nodes.push(Node {
                operation,
                next: following,
            });
            next = Some(index);
        }
        next
    }

    fn reachable(&self, roots: impl IntoIterator<Item = usize>) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut pending: Vec<_> = roots.into_iter().collect();
        while let Some(node) = pending.pop() {
            if seen.insert(node) {
                pending.extend(self.nodes[node].next.iter().copied());
            }
        }
        seen
    }

    fn dominators(&self, entry: usize) -> Vec<HashSet<usize>> {
        let reachable = self.reachable([entry]);
        let mut predecessors = vec![Vec::new(); self.nodes.len()];
        for &index in &reachable {
            for &next in &self.nodes[index].next {
                predecessors[next].push(index);
            }
        }
        let mut dom = vec![HashSet::new(); self.nodes.len()];
        for &index in &reachable {
            dom[index] = reachable.clone();
        }
        dom[entry] = HashSet::from([entry]);
        loop {
            let mut changed = false;
            // Nodes were appended in reverse execution order.
            for index in (0..self.nodes.len()).rev() {
                if index == entry || !reachable.contains(&index) {
                    continue;
                }
                let mut incoming = predecessors[index].iter();
                let mut value = incoming.next().map(|p| dom[*p].clone()).unwrap_or_default();
                for p in incoming {
                    value.retain(|n| dom[*p].contains(n));
                }
                value.insert(index);
                if dom[index] != value {
                    dom[index] = value;
                    changed = true;
                }
            }
            if !changed {
                return dom;
            }
        }
    }
}

#[derive(Hash, PartialEq, Eq)]
enum Key {
    Constant(u64),
    Frame,
    Address(usize, u32),
    Load(usize, u8, bool),
}
#[derive(Default)]
struct Numbers {
    next: usize,
    canonical: HashMap<Key, usize>,
    addresses: HashMap<usize, (usize, u32)>,
    nonvolatile: HashSet<usize>,
}
impl Numbers {
    fn fresh(&mut self) -> usize {
        let value = self.next;
        self.next += 1;
        value
    }
    fn key(&mut self, key: Key) -> usize {
        if let Some(value) = self.canonical.get(&key) {
            return *value;
        }
        let value = self.fresh();
        self.canonical.insert(key, value);
        value
    }
    fn address(&mut self, base: usize, offset: u32) -> usize {
        let (base, previous) = self.addresses.get(&base).copied().unwrap_or((base, 0));
        let offset = previous.wrapping_add(offset);
        if offset == 0 {
            return base;
        }
        let value = self.key(Key::Address(base, offset));
        self.addresses.insert(value, (base, offset));
        value
    }
    fn value(&mut self, value: Value, values: &mut HashMap<usize, usize>) -> usize {
        match value.source {
            Source::Constant(bits) => self.key(Key::Constant(bits)),
            Source::Register(id) => *values.entry(id).or_insert_with(|| self.fresh()),
        }
    }
}

fn result(op: &Operation) -> Option<Value> {
    match op {
        Operation::Parameter { result, .. }
        | Operation::Copy { result, .. }
        | Operation::Convert { result, .. }
        | Operation::Load { result, .. }
        | Operation::Offset { result, .. }
        | Operation::Address { result, .. }
        | Operation::FrameAddress { result, .. }
        | Operation::Binary { result, .. } => Some(*result),
        Operation::Call { result, .. } => *result,
        _ => None,
    }
}

struct Numbering<'a> {
    numbers: Numbers,
    mutable: HashSet<usize>,
    readonly_memory: bool,
    ordinary_pointers: &'a HashSet<usize>,
    sites: &'a Sites,
    groups: HashMap<usize, Vec<usize>>,
}
impl Numbering<'_> {
    fn invalidate(&mut self, values: &mut HashMap<usize, usize>) {
        for id in &self.mutable {
            values.insert(*id, self.numbers.fresh());
        }
    }
    fn scan(&mut self, operations: &[Operation], values: &mut HashMap<usize, usize>) {
        for op in operations {
            if let Operation::Branch {
                then_body,
                else_body,
                ..
            } = op
            {
                self.scan(then_body, &mut values.clone());
                self.scan(else_body, &mut values.clone());
                self.invalidate(values);
                continue;
            }
            if matches!(op, Operation::Label(_)) {
                self.invalidate(values);
            }
            let Some(Value {
                source: Source::Register(id),
                ty,
            }) = result(op)
            else {
                continue;
            };
            if let Operation::Convert { value, .. } = op {
                if self.sites.definitions.contains_key(&id) && !self.sites.explicit_word_cast(id) {
                    let key = self.numbers.value(*value, values);
                    self.groups.entry(key).or_default().push(id);
                }
            }
            let value = match op {
                Operation::Copy { value, .. } => self.numbers.value(*value, values),
                Operation::FrameAddress { offset, .. } => {
                    let base = self.numbers.key(Key::Frame);
                    self.numbers.address(base, *offset as i32 as u32)
                }
                Operation::Offset { pointer, bytes, .. } => {
                    let base = self.numbers.value(*pointer, values);
                    self.numbers.address(base, *bytes)
                }
                Operation::Binary {
                    operator,
                    left,
                    right,
                    ..
                } if matches!(ty, Type::Pointer(_) | Type::StructPointer { .. }) => {
                    match (operator, right.source) {
                        (BinaryOperator::Add | BinaryOperator::Subtract, Source::Constant(n)) => {
                            let base = self.numbers.value(*left, values);
                            self.numbers.address(
                                base,
                                if *operator == BinaryOperator::Subtract {
                                    (n as u32).wrapping_neg()
                                } else {
                                    n as u32
                                },
                            )
                        }
                        _ => self.numbers.fresh(),
                    }
                }
                Operation::Binary {
                    operator: BinaryOperator::Add | BinaryOperator::Subtract,
                    left,
                    right:
                        Value {
                            source: Source::Constant(0),
                            ..
                        },
                    ..
                } if !wide(ty) => self.numbers.value(*left, values),
                Operation::Binary {
                    operator: BinaryOperator::Add,
                    left:
                        Value {
                            source: Source::Constant(0),
                            ..
                        },
                    right,
                    ..
                } if !wide(ty) => self.numbers.value(*right, values),
                Operation::Load { address, .. } if self.readonly_memory => {
                    let base = match address.base {
                        address::AddressBase::Value(value) => self.numbers.value(value, values),
                        address::AddressBase::Frame => self.numbers.key(Key::Frame),
                    };
                    let pointer = self.numbers.address(base, address.offset as i32 as u32);
                    let base = self
                        .numbers
                        .addresses
                        .get(&pointer)
                        .map_or(pointer, |a| a.0);
                    if self.numbers.nonvolatile.contains(&base) {
                        self.numbers
                            .key(Key::Load(pointer, ty.width(), ty.is_signed()))
                    } else {
                        self.numbers.fresh()
                    }
                }
                _ => self.numbers.fresh(),
            };
            if matches!(op, Operation::Parameter { .. }) && self.ordinary_pointers.contains(&id) {
                self.numbers.nonvolatile.insert(value);
            }
            values.insert(id, value);
        }
    }
}

impl Graph<'_> {
    pub(super) fn mark_word_cast_promotion(&mut self, value: Value, cast: bool) {
        if self.computed_unsigned_addend_zero_extends && cast {
            if let Source::Register(id) = value.source {
                if let Some(site) = self.promotion_sites.definitions.get_mut(&id) {
                    site.1 = true;
                }
            }
        }
    }

    pub(super) fn find_shared_word_promotions(&self) -> Sharing {
        let mut sharing = Sharing::default();
        if !self.computed_unsigned_addend_zero_extends
            || self.optimization < mwcc_versions::Optimization::O2
        {
            return sharing;
        }
        let Some((flow, entry)) = Flow::new(&self.operations) else {
            return sharing;
        };
        let mut writes = HashMap::<usize, usize>::new();
        let mut definitions = HashMap::new();
        let mut readonly_memory = true;
        for (index, node) in flow.nodes.iter().enumerate() {
            if matches!(
                node.operation,
                Operation::Store { .. } | Operation::Call { .. }
            ) {
                readonly_memory = false;
            }
            if let Some(Value {
                source: Source::Register(id),
                ..
            }) = result(node.operation)
            {
                *writes.entry(id).or_default() += 1;
                definitions.insert(id, index);
            }
        }
        let mut numbering = Numbering {
            numbers: Numbers::default(),
            mutable: writes
                .into_iter()
                .filter_map(|(id, count)| (count > 1).then_some(id))
                .collect(),
            readonly_memory,
            ordinary_pointers: &self.nonvolatile_pointer_values,
            sites: &self.promotion_sites,
            groups: HashMap::new(),
        };
        numbering.scan(&self.operations, &mut HashMap::new());
        let groups: Vec<_> = numbering
            .groups
            .into_values()
            .filter(|g| g.len() > 1)
            .collect();
        if groups.is_empty() {
            return sharing;
        }
        let dominators = flow.dominators(entry);
        for group in groups {
            // Definitions are collected in source execution order. A named
            // wide promotion is one definition even when it has many uses.
            let first = group[0];
            let first_node = definitions[&first];
            let site = self.promotion_sites.definitions[&first].0;
            sharing.full.extend(group.iter().copied());
            if self.named_values.contains(&first)
                || group
                    .iter()
                    .skip(1)
                    .any(|id| self.promotion_sites.definitions[id].0 == site)
                || group
                    .iter()
                    .any(|id| !dominators[definitions[id]].contains(&first_node))
                || flow
                    .reachable(flow.nodes[first_node].next.iter().copied())
                    .contains(&first_node)
            {
                continue;
            }
            sharing.full.remove(&first);
            sharing.zero.insert(first);
        }
        sharing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition() -> Condition {
        Condition::Value(Value {
            ty: Type::Int,
            source: Source::Register(0),
        })
    }

    #[test]
    fn alternate_arm_does_not_dominate_the_join() {
        let operations = vec![
            Operation::Label(0),
            Operation::Branch {
                condition: condition(),
                then_body: vec![Operation::Label(1)],
                else_body: vec![Operation::Label(2)],
            },
            Operation::Label(3),
        ];
        let (flow, entry) = Flow::new(&operations).unwrap();
        let dom = flow.dominators(entry);
        let join = flow.labels[&3];
        assert!(dom[join].contains(&flow.labels[&0]));
        assert!(!dom[join].contains(&flow.labels[&1]));
        assert!(!dom[join].contains(&flow.labels[&2]));
        assert!(!flow
            .reachable(flow.nodes[join].next.iter().copied())
            .contains(&join));
    }

    #[test]
    fn loop_body_is_cyclic_and_does_not_dominate_the_exit() {
        let operations = vec![
            Operation::Label(0),
            Operation::Label(1),
            Operation::BranchIf {
                condition: condition(),
                target: 3,
                on_true: false,
            },
            Operation::Label(2),
            Operation::Jump(1),
            Operation::Label(3),
        ];
        let (flow, entry) = Flow::new(&operations).unwrap();
        let dom = flow.dominators(entry);
        let body = flow.labels[&2];
        assert!(flow
            .reachable(flow.nodes[body].next.iter().copied())
            .contains(&body));
        assert!(!dom[flow.labels[&3]].contains(&body));
        assert!(dom[body].contains(&flow.labels[&0]));
        assert!(dom[flow.labels[&3]].contains(&flow.labels[&0]));
    }
}
