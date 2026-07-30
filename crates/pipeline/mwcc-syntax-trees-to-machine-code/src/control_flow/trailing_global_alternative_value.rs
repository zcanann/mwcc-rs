//! Logical values whose final alternative reads a global scalar or element.
//!
//! MWCC gives the leading source group one shared true materialization, then
//! emits a separate true/false diamond for the final global comparison.  This
//! preserves the nested source expression's value boundaries without creating
//! a complete intermediate boolean between the groups.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Type;

struct TrailingGlobalAlternative<'a> {
    prefix: Vec<&'a Expression>,
    final_term: &'a Expression,
    global_candidates: [Option<&'a str>; 2],
}

impl Generator {
    pub(crate) fn try_emit_trailing_global_alternative_value(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(alternative) = trailing_global_alternative(expression) else {
            return Ok(false);
        };
        let has_global_value = alternative
            .global_candidates
            .into_iter()
            .flatten()
            .any(|name| {
                !self.locations.contains_key(name)
                    && !self.volatile_globals.contains(name)
                    && matches!(
                        self.globals.get(name),
                        Some(
                            Type::Int
                                | Type::UnsignedInt
                                | Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                                | Type::Pointer(_)
                                | Type::StructPointer { .. }
                        )
                    )
            });
        if !has_global_value {
            return Ok(false);
        }

        let suffix = self.fresh_label();
        let prefix_true = self.fresh_label();
        let false_value = self.fresh_label();
        let join = self.fresh_label();
        let last_prefix = alternative.prefix.len() - 1;
        for (index, term) in alternative.prefix.into_iter().enumerate() {
            let (false_options, condition_bit) = self.emit_condition_test(term)?;
            self.emit_branch_conditional_to(
                if index == last_prefix {
                    false_options
                } else {
                    false_options ^ 8
                },
                condition_bit,
                if index == last_prefix {
                    suffix
                } else {
                    prefix_true
                },
            );
        }

        self.bind_label(prefix_true);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
        self.emit_branch_to(join);

        self.bind_label(suffix);
        let (false_options, condition_bit) = self.emit_condition_test(alternative.final_term)?;
        self.emit_branch_conditional_to(false_options, condition_bit, false_value);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
        self.emit_branch_to(join);
        self.bind_label(false_value);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.bind_label(join);
        if destination != GENERAL_SCRATCH {
            self.output
                .instructions
                .push(Instruction::move_register(destination, GENERAL_SCRATCH));
        }
        Ok(true)
    }
}

fn trailing_global_alternative(expression: &Expression) -> Option<TrailingGlobalAlternative<'_>> {
    let terms = super::logical_value::logical_or_terms(expression)?;
    if terms.len() < 4 {
        return None;
    }
    let final_term = *terms.last()?;
    let global_candidates = global_value_equality(final_term)?;
    Some(TrailingGlobalAlternative {
        prefix: terms[..terms.len() - 1].to_vec(),
        final_term,
        global_candidates,
    })
}

fn global_value_equality(expression: &Expression) -> Option<[Option<&str>; 2]> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    fn indexed_global(expression: &Expression) -> Option<&str> {
        let Expression::Index { base, index } = expression else {
            return None;
        };
        let Expression::Variable(global) = base.as_ref() else {
            return None;
        };
        (constant_value(index)? == 0).then_some(global)
    }
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(left), Expression::Variable(right)) => {
            Some([Some(left), Some(right)])
        }
        (Expression::Variable(_), indexed) => Some([Some(indexed_global(indexed)?), None]),
        (indexed, Expression::Variable(_)) => Some([Some(indexed_global(indexed)?), None]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn equal(left: &str, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::Variable(left.into())),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_a_four_term_or_ending_in_a_global_element_equality() {
        let prefix = or(
            or(
                equal("command", Expression::IntegerLiteral(1)),
                equal("command", Expression::IntegerLiteral(4)),
            ),
            equal("command", Expression::IntegerLiteral(14)),
        );
        let expression = or(
            prefix,
            equal(
                "command",
                Expression::Index {
                    base: Box::new(Expression::Variable("dma_command".into())),
                    index: Box::new(Expression::IntegerLiteral(0)),
                },
            ),
        );
        let shape = trailing_global_alternative(&expression).expect("trailing global alternative");
        assert_eq!(shape.prefix.len(), 3);
        assert_eq!(shape.global_candidates, [Some("dma_command"), None]);
    }

    #[test]
    fn rejects_short_or_literal_suffixes() {
        let short = or(
            or(
                equal("command", Expression::IntegerLiteral(1)),
                equal("command", Expression::IntegerLiteral(4)),
            ),
            equal("command", Expression::Variable("dma_command".into())),
        );
        assert!(trailing_global_alternative(&short).is_none());

        let literal_suffix = or(short, equal("command", Expression::IntegerLiteral(14)));
        assert!(trailing_global_alternative(&literal_suffix).is_none());
    }
}
