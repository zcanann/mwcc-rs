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

#[cfg(test)]
mod tests {
    use super::*;

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
