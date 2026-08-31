//! Two-member call arguments sharing one retained base.
//!
//! Build 163 loads the second argument first when both are byte members of the
//! same callee-saved object and the second feeds a bit-field extraction. The
//! extraction itself remains after both loads, filling the first load's latency
//! slot. This pass verifies each complete call prefix before changing it.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_shared_member_arguments(&mut self, function: &Function) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let calls = recognize(function);
        if calls.is_empty() {
            return;
        }
        let mut pairs = Vec::new();
        let mut cursor = 0;
        for call_plan in calls {
            let Some(base_register) = self.lookup_general(call_plan.base) else {
                return;
            };
            if call_plan.has_trailing_leaf {
                // Its load/copy/load/rotate order is already emitted directly.
                // The physical copy spelling is repaired after allocation.
                continue;
            }
            let Some((call, first)) = self.output.instructions[cursor..]
                .iter()
                .enumerate()
                .filter_map(|(relative, instruction)| {
                    let call = cursor + relative;
                    matches!(instruction, Instruction::BranchAndLink { target } if target == call_plan.name)
                        .then(|| call.checked_sub(3).map(|first| (call, first)))
                        .flatten()
                })
                .find(|(_, first)| {
                    matches!(
                        self.output.instructions.get(*first..*first + 3),
                        Some([
                            Instruction::LoadByteZero { d: 3, a: first_base, .. },
                            Instruction::LoadByteZero { d: 4, a: second_base, .. },
                            Instruction::RotateAndMask { a: 4, s: 4, .. },
                        ]) if *first_base == base_register && *second_base == base_register
                    )
                })
            else {
                return;
            };
            if self.output.relocations.iter().any(|relocation| {
                relocation.instruction_index == first
                    || relocation.instruction_index == first + 1
            }) {
                return;
            }
            pairs.push(first);
            cursor = call + 1;
        }
        for first in pairs {
            self.output.instructions.swap(first, first + 1);
            self.labels.moved_before(first + 1, first);
        }
    }

    /// Allocation applies build 163's general `addi d,s,0` materialization
    /// convention after the structured scheduler has formed this packet. The
    /// measured independent third-argument latency fill is an exception and
    /// retains `mr`, so repair it on the final physical instruction stream.
    pub(crate) fn normalize_structured_shared_member_argument_copies(
        &mut self,
        function: &Function,
    ) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let call_names = recognize(function)
            .into_iter()
            .filter(|plan| plan.has_trailing_leaf)
            .map(|plan| plan.name)
            .collect::<std::collections::HashSet<_>>();
        if call_names.is_empty() {
            return;
        }
        for first in 0..self.output.instructions.len().saturating_sub(4) {
            let source = match &self.output.instructions[first..first + 5] {
                [
                    Instruction::LoadByteZero { d: 4, a: second_base, .. },
                    Instruction::AddImmediate { d: 5, a: source, immediate: 0 },
                    Instruction::LoadByteZero { d: 3, a: first_base, .. },
                    Instruction::RotateAndMask { a: 4, s: 4, .. },
                    Instruction::BranchAndLink { target },
                ] if first_base == second_base
                    && *source != 0
                    && call_names.contains(target.as_str()) => *source,
                _ => continue,
            };
            self.output.instructions[first + 1] = Instruction::move_register(5, source);
        }
    }
}

struct SharedMemberCall<'a> {
    name: &'a str,
    base: &'a str,
    has_trailing_leaf: bool,
}

fn recognize(function: &Function) -> Vec<SharedMemberCall<'_>> {
    let mut calls = Vec::new();
    collect_statement_calls(&function.statements, &mut calls);
    calls
}

fn collect_statement_calls<'a>(
    statements: &'a [Statement],
    calls: &mut Vec<SharedMemberCall<'a>>,
) {
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_expression_calls(condition, calls);
                collect_statement_calls(then_body, calls);
                collect_statement_calls(else_body, calls);
            }
            Statement::Store { target, value } => {
                collect_expression_calls(target, calls);
                collect_expression_calls(value, calls);
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => collect_expression_calls(value, calls),
            _ => {}
        }
    }
}

fn collect_expression_calls<'a>(
    expression: &'a Expression,
    calls: &mut Vec<SharedMemberCall<'a>>,
) {
    match expression {
        Expression::Call { name, arguments } => {
            if let Some((base, has_trailing_leaf)) =
                shared_byte_and_bitfield_arguments(arguments)
            {
                calls.push(SharedMemberCall {
                    name,
                    base,
                    has_trailing_leaf,
                });
            }
            for argument in arguments {
                collect_expression_calls(argument, calls);
            }
        }
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            collect_expression_calls(left, calls);
            collect_expression_calls(right, calls);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::PostStep {
            target: operand, ..
        } => collect_expression_calls(operand, calls),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_expression_calls(condition, calls);
            collect_expression_calls(when_true, calls);
            collect_expression_calls(when_false, calls);
        }
        _ => {}
    }
}

fn shared_byte_and_bitfield_arguments(arguments: &[Expression]) -> Option<(&str, bool)> {
    let (first, bit_field, has_trailing_leaf) = match arguments {
        [first, bit_field] => (first, bit_field, false),
        [first, bit_field, Expression::Variable(_)] => (first, bit_field, true),
        _ => return None,
    };
    let Expression::Member {
        base: first_base,
        member_type: Type::UnsignedChar,
        index_stride: None,
        ..
    } = first
    else {
        return None;
    };
    let Expression::BitFieldRead { storage, .. } = bit_field else {
        return None;
    };
    let Expression::Member {
        base: second_base,
        member_type: Type::UnsignedChar,
        index_stride: None,
        ..
    } = storage.as_ref()
    else {
        return None;
    };
    let (Expression::Variable(first_name), Expression::Variable(second_name)) =
        (first_base.as_ref(), second_base.as_ref())
    else {
        return None;
    };
    (first_name == second_name).then_some((first_name.as_str(), has_trailing_leaf))
}
