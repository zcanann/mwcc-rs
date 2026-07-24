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
        for (call_name, base_name) in calls {
            let Some(base_register) = self.lookup_general(base_name) else {
                return;
            };
            let Some((call, first)) = self.output.instructions[cursor..]
                .iter()
                .enumerate()
                .filter_map(|(relative, instruction)| {
                    let call = cursor + relative;
                    matches!(instruction, Instruction::BranchAndLink { target } if target == call_name)
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
}

fn recognize(function: &Function) -> Vec<(&str, &str)> {
    let mut calls = Vec::new();
    collect_statement_calls(&function.statements, &mut calls);
    calls
}

fn collect_statement_calls<'a>(
    statements: &'a [Statement],
    calls: &mut Vec<(&'a str, &'a str)>,
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
    calls: &mut Vec<(&'a str, &'a str)>,
) {
    match expression {
        Expression::Call { name, arguments } => {
            if let Some(base) = shared_byte_and_bitfield_arguments(arguments) {
                calls.push((name, base));
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

fn shared_byte_and_bitfield_arguments(arguments: &[Expression]) -> Option<&str> {
    let [
        Expression::Member {
            base: first_base,
            member_type: Type::UnsignedChar,
            index_stride: None,
            ..
        },
        Expression::BitFieldRead { storage, .. },
    ] = arguments
    else {
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
    (first_name == second_name).then_some(first_name.as_str())
}
