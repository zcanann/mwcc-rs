//! Latency scheduling for an entry-initialized saved floating local.
//!
//! A call-produced float that survives later calls already has an allocator
//! home. This pass only reconciles the two instruction-order details MWCC
//! exposes around that semantic plan: the second same-base argument consumes
//! the saved GPR alias, and the first independent post-call word load fills the
//! call-result latency slot before the unary float operation.

#[allow(unused_imports)]
use super::*;

fn root_variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Member { base, .. }
        | Expression::MemberAddress { base, .. }
        | Expression::AddressOf { operand: base }
        | Expression::Dereference { pointer: base }
        | Expression::Cast { operand: base, .. } => root_variable(base),
        Expression::Index { base, .. } => root_variable(base),
        _ => None,
    }
}

fn entry_general_register(function: &Function, name: &str) -> Option<u8> {
    let mut next = Eabi::FIRST_GENERAL_ARGUMENT;
    for parameter in &function.parameters {
        if class_of(parameter.parameter_type).ok()? == ValueClass::General {
            if parameter.name == name {
                return Some(next);
            }
            next = next.checked_add(1)?;
        }
    }
    None
}

impl Generator {
    pub(super) fn schedule_entry_initialized_saved_float(&mut self, function: &Function) {
        let shape = function.locals.iter().find_map(|local| {
            if !matches!(local.declared_type, Type::Float | Type::Double) {
                return None;
            }
            let Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } = local.initializer.as_ref()?
            else {
                return None;
            };
            let Expression::Call { name, arguments } = operand.as_ref() else {
                return None;
            };
            let [first, second] = arguments.as_slice() else {
                return None;
            };
            let root = root_variable(first)?;
            (root_variable(second) == Some(root)).then_some((name.as_str(), root))
        });
        let Some((callee, root)) = shape else {
            return;
        };
        let Some(entry) = entry_general_register(function, root) else {
            return;
        };
        let Some(saved) = self.lookup_general(root).filter(|saved| *saved >= 14 && *saved != entry)
        else {
            return;
        };
        let Some(call) = self.output.instructions.iter().position(
            |instruction| matches!(instruction, Instruction::BranchAndLink { target } if target == callee),
        ) else {
            return;
        };
        if call >= 2 {
            let (prefix, _) = self.output.instructions.split_at_mut(call);
            let first = prefix.len() - 2;
            let matching_pair = match (&prefix[first], &prefix[first + 1]) {
                (
                    Instruction::LoadFloatSingle { d: 1, a: first_base, .. },
                    Instruction::LoadFloatSingle { d: 2, a: second_base, .. },
                )
                | (
                    Instruction::LoadFloatDouble { d: 1, a: first_base, .. },
                    Instruction::LoadFloatDouble { d: 2, a: second_base, .. },
                ) => *first_base == entry && *second_base == entry,
                _ => false,
            };
            if matching_pair {
                match &mut prefix[first + 1] {
                    Instruction::LoadFloatSingle { a, .. }
                    | Instruction::LoadFloatDouble { a, .. } => *a = saved,
                    _ => unreachable!("the pair was matched above"),
                }
            }
        }
        if matches!(self.output.instructions.get(call + 1), Some(Instruction::FloatNegate { .. }))
            && matches!(self.output.instructions.get(call + 2), Some(Instruction::LoadWord { .. }))
        {
            self.output.instructions.swap(call + 1, call + 2);
        }
    }
}
