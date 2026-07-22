//! Final-use call-argument encodings in allocator-owned structured bodies.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

pub(super) fn transient_call_argument_register(
    statements: &[Statement],
    candidate: &str,
) -> Option<u8> {
    statements
        .iter()
        .find_map(|statement| statement_call_argument_index(statement, candidate))
        .and_then(|index| u8::try_from(index).ok())
        .and_then(|index| Eabi::FIRST_GENERAL_ARGUMENT.checked_add(index))
        .filter(|register| *register <= 10)
}

fn statement_call_argument_index(statement: &Statement, candidate: &str) -> Option<usize> {
    match statement {
        Statement::Store { target, value } => expression_call_argument_index(target, candidate)
            .or_else(|| expression_call_argument_index(value, candidate)),
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => expression_call_argument_index(value, candidate),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => expression_call_argument_index(condition, candidate).or_else(|| {
            then_body
                .iter()
                .chain(else_body)
                .find_map(|statement| statement_call_argument_index(statement, candidate))
        }),
        _ => None,
    }
}

fn expression_call_argument_index(expression: &Expression, candidate: &str) -> Option<usize> {
    match expression {
        Expression::Call { arguments, .. } => arguments.iter().position(
            |argument| matches!(argument, Expression::Variable(name) if name == candidate),
        ),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => expression_call_argument_index(left, candidate)
            .or_else(|| expression_call_argument_index(right, candidate)),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::PostStep {
            target: operand, ..
        } => expression_call_argument_index(operand, candidate),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => expression_call_argument_index(condition, candidate)
            .or_else(|| expression_call_argument_index(when_true, candidate))
            .or_else(|| expression_call_argument_index(when_false, candidate)),
        _ => None,
    }
}

impl Generator {
    /// Build 163 keeps the power-of-two product in r3 while forming a second
    /// call argument such as `consume(data, length * 8 + 1)`. The generic
    /// immediate selector coalesces both operations into the argument home;
    /// split the producer lifetime so allocation can retain MWCC's intermediate.
    pub(super) fn stage_legacy_shift_add_call_argument(
        &mut self,
        statement: &Statement,
        remaining: &[Statement],
        emitted_start: usize,
    ) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Statement::Assign { name, value } = statement else {
            return;
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = value
        else {
            return;
        };
        if !matches!(
            (left.as_ref(), right.as_ref()),
            (
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    right,
                    ..
                },
                Expression::IntegerLiteral(_)
            ) if crate::analysis::constant_value(right).is_some_and(|factor| {
                factor > 1 && (factor as u64).is_power_of_two()
            })
        ) {
            return;
        }
        if remaining
            .first()
            .and_then(|next| statement_call_argument_index(next, name))
            != Some(1)
        {
            return;
        }
        let Some(home) = self.lookup_general(name) else {
            return;
        };
        if !is_coalesced_shift_add_window(&self.output.instructions[emitted_start..], home) {
            return;
        }
        let staged = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        let [Instruction::ShiftLeftImmediate { a, .. }, Instruction::AddImmediate { a: source, .. }] =
            &mut self.output.instructions[emitted_start..]
        else {
            unreachable!("window checked above");
        };
        *a = staged;
        *source = staged;
    }

    /// Build 163 spells the final multi-argument forwarding of a deferred local
    /// home as `addi d,s,0`, while earlier uses and entry-initialized locals
    /// remain `mr`. Selection cannot infer this provenance from the virtual
    /// register number, so the structured statement owner applies the encoding
    /// only after proving this call is the deferred local's final use.
    pub(super) fn schedule_dying_structured_local_argument(
        &mut self,
        statement: &Statement,
        remaining: &[Statement],
        function: &Function,
        emitted_start: usize,
    ) {
        if self.behavior.materialization_copy_style
            != mwcc_versions::MaterializationCopyStyle::AddImmediateZero
        {
            return;
        }
        let Some(name) = dying_first_local_argument(statement, remaining, &self.known_locals)
        else {
            return;
        };
        if !function
            .locals
            .iter()
            .any(|local| local.name == name && local.initializer.is_none())
        {
            return;
        }
        let Some(source) = self.lookup_general(name) else {
            return;
        };
        let candidates: Vec<usize> = self.output.instructions[emitted_start..]
            .iter()
            .enumerate()
            .filter_map(|(offset, instruction)| {
                matches!(instruction, Instruction::Or { a: 3, s, b } if *s == source && *b == source)
                    .then_some(emitted_start + offset)
            })
            .collect();
        let [copy] = candidates.as_slice() else {
            return;
        };
        self.output.instructions[*copy] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT,
            a: source,
            immediate: 0,
        };
    }
}

fn dying_first_local_argument<'a>(
    statement: &'a Statement,
    remaining: &[Statement],
    known_locals: &std::collections::HashSet<String>,
) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
        return None;
    };
    let [Expression::Variable(name), _, ..] = arguments.as_slice() else {
        return None;
    };
    (known_locals.contains(name) && !body_uses_local(remaining, name)).then_some(name)
}

fn is_coalesced_shift_add_window(instructions: &[Instruction], home: u8) -> bool {
    matches!(
        instructions,
        [
            Instruction::ShiftLeftImmediate { a, .. },
            Instruction::AddImmediate {
                d,
                a: source,
                ..
            }
        ] if *a == home && *d == home && *source == home
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_coalesced_shift_add_argument() {
        let instructions = [
            Instruction::ShiftLeftImmediate {
                a: 40,
                s: 41,
                shift: 3,
            },
            Instruction::AddImmediate {
                d: 40,
                a: 40,
                immediate: 1,
            },
        ];
        assert!(is_coalesced_shift_add_window(&instructions, 40));
        assert!(!is_coalesced_shift_add_window(&instructions, 41));
    }

    fn call(arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments,
        })
    }

    #[test]
    fn distinguishes_final_multi_argument_local_forwarding() {
        let current = call(vec![
            Expression::Variable("local".into()),
            Expression::IntegerLiteral(0),
        ]);
        let known = std::collections::HashSet::from(["local".to_string()]);
        assert_eq!(
            dying_first_local_argument(&current, &[], &known),
            Some("local")
        );

        let later_use = vec![call(vec![Expression::Variable("local".into())])];
        assert_eq!(
            dying_first_local_argument(&current, &later_use, &known),
            None
        );
        assert_eq!(dying_first_local_argument(&later_use[0], &[], &known), None);
    }

    #[test]
    fn selects_the_eabi_register_for_a_forwarded_argument() {
        let statement = call(vec![
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::Variable("length".into()),
        ]);
        assert_eq!(
            transient_call_argument_register(&[statement], "length"),
            Some(6),
        );
    }
}
