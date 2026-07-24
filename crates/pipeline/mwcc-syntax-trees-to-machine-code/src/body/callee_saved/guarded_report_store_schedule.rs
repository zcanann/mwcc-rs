//! Scheduling for a guarded diagnostic pair before a call-result member store.
//!
//! In a linkage-first two-home frame, MWCC uses an OR copy for the first
//! parameter's home and fills the split-address dependency gap of the one-string
//! variadic report with `crclr`. The generic structured emitter has already
//! proved all value lifetimes; this pass only selects those measured encodings.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_guarded_report_store(&mut self, function: &Function) {
        let Some(report) = recognize(function) else {
            return;
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let mut home_copies = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                match instruction {
                    Instruction::AddImmediate {
                        d,
                        a: 3,
                        immediate: 0,
                    } if (14..=31).contains(d) => Some((index, *d)),
                    _ => None,
                }
            });
        let Some((home_copy, home)) = home_copies.next() else {
            return;
        };
        if home_copies.next().is_some() {
            return;
        }
        let Some(call) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::BranchAndLink { target } if target == report)
        }) else {
            return;
        };
        let Some(prefix_start) = call.checked_sub(3) else {
            return;
        };
        if !matches!(
            &self.output.instructions[prefix_start..call],
            [
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::ConditionRegisterClear { d: 6 },
            ]
        ) {
            return;
        }

        self.output.instructions[home_copy] = Instruction::move_register(home, 3);
        let clear = call - 1;
        let low = call - 2;
        self.output.instructions.swap(low, clear);
        self.labels.moved_before(clear, low);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == low => clear,
                index if index == clear => low,
                index => index,
            };
        }
    }
}

fn recognize(function: &Function) -> Option<&str> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object, source] = function.parameters.as_slice() else {
        return None;
    };
    let [
        Statement::If {
            condition,
            then_body,
            else_body,
        },
        Statement::Store { target, value },
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let guarded_offset = nonnull_member_offset(condition, &object.name)?;
    let [
        Statement::Expression(Expression::Call {
            name: report,
            arguments: report_arguments,
        }),
        Statement::Expression(Expression::Call {
            arguments: assertion_arguments,
            ..
        }),
    ] = then_body.as_slice()
    else {
        return None;
    };
    if !matches!(report_arguments.as_slice(), [Expression::StringLiteral(_)])
        || !matches!(assertion_arguments.as_slice(), [Expression::StringLiteral(_), line, Expression::StringLiteral(_)] if constant_value(line).is_some())
        || member_offset(target, &object.name) != Some(guarded_offset)
        || !matches!(value,
            Expression::Call { arguments, .. }
                if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == &source.name))
    {
        return None;
    }
    Some(report)
}

fn nonnull_member_offset(expression: &Expression, base: &str) -> Option<u32> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    matches!(right.as_ref(), Expression::IntegerLiteral(0))
        .then(|| member_offset(left, base))?
}

fn member_offset(expression: &Expression, base: &str) -> Option<u32> {
    let Expression::Member {
        base: member_base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    matches!(member_base.as_ref(), Expression::Variable(name) if name == base).then_some(*offset)
}
