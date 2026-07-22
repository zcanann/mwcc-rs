//! Final-use call-argument encodings in allocator-owned structured bodies.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

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
        assert_eq!(
            dying_first_local_argument(&later_use[0], &[], &known),
            None
        );
    }
}
