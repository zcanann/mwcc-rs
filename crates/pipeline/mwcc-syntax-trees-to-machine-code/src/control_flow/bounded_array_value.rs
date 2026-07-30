//! Logical values with a dense prefix followed by a bounded table search.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationKind;
use mwcc_syntax_trees::Pointee;
use mwcc_versions::GlobalAddressing;

struct BoundedArrayAlternatives<'a> {
    prefix: Vec<&'a Expression>,
    needle: &'a str,
    global: &'a str,
    count: usize,
    pointee: Pointee,
}

impl Generator {
    pub(crate) fn try_emit_bounded_array_alternative_value(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(alternatives) = self.bounded_array_alternatives(expression) else {
            return Ok(false);
        };
        let needle = self.lookup_general(alternatives.needle).ok_or_else(|| {
            Diagnostic::error("a bounded array predicate needs a register-resident needle")
        })?;
        self.prefer_virtual_general(needle, 4);

        let table = self.fresh_virtual_general_preferring(3);
        let table_start = self.fresh_label();
        let prefix_true = self.fresh_label();
        let join = self.fresh_label();
        let last_prefix = alternatives.prefix.len() - 1;
        for (index, term) in alternatives.prefix.into_iter().enumerate() {
            let (false_options, condition_bit) = self.emit_condition_test(term)?;
            self.emit_branch_conditional_to(
                if index == last_prefix {
                    false_options
                } else {
                    false_options ^ 8
                },
                condition_bit,
                if index == last_prefix {
                    table_start
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

        self.bind_label(table_start);
        self.emit_address_high(table, alternatives.global);
        for index in 0..alternatives.count {
            let offset = if index == 0 {
                self.record_relocation(RelocationKind::Addr16Lo, alternatives.global);
                0
            } else {
                i16::from(alternatives.pointee.size())
            };
            if index + 1 == alternatives.count {
                self.output.instructions.push(Instruction::LoadWord {
                    d: GENERAL_SCRATCH,
                    a: table,
                    offset,
                });
            } else {
                self.output
                    .instructions
                    .push(Instruction::LoadWordWithUpdate {
                        d: GENERAL_SCRATCH,
                        a: table,
                        offset,
                    });
            }
            self.output
                .instructions
                .push(Instruction::CompareLogicalWord {
                    a: needle,
                    b: GENERAL_SCRATCH,
                });
            let next = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, next);
            self.output
                .instructions
                .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
            self.emit_branch_to(join);
            self.bind_label(next);
        }
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

    fn bounded_array_alternatives<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<BoundedArrayAlternatives<'a>> {
        let shape = bounded_array_alternative_shape(expression)?;
        let total_size = self.global_array_sizes.get(shape.global).copied()?;
        let pointee = crate::expressions::pointee_of_type(*self.globals.get(shape.global)?)?;
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small
            || !matches!(
                pointee,
                Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer
            )
            || pointee.size() != 4
            || shape.count > usize::try_from(total_size / u32::from(pointee.size())).ok()?
            || self.volatile_globals.contains(shape.global)
        {
            return None;
        }
        Some(BoundedArrayAlternatives {
            prefix: shape.prefix,
            needle: shape.needle,
            global: shape.global,
            count: shape.count,
            pointee,
        })
    }
}

struct BoundedArrayAlternativeShape<'a> {
    prefix: Vec<&'a Expression>,
    needle: &'a str,
    global: &'a str,
    count: usize,
}

fn bounded_array_alternative_shape(
    expression: &Expression,
) -> Option<BoundedArrayAlternativeShape<'_>> {
    let terms = super::logical_value::logical_or_terms(expression)?;
    let first_array = terms
        .iter()
        .position(|term| array_equality(term).is_some())?;
    if first_array == 0 {
        return None;
    }
    let (needle, global, first_index) = array_equality(terms[first_array])?;
    if first_index != 0 {
        return None;
    }
    for (expected, term) in terms[first_array..].iter().enumerate() {
        let (term_needle, term_global, index) = array_equality(term)?;
        if term_needle != needle || term_global != global || index != expected {
            return None;
        }
    }
    let count = terms.len() - first_array;
    if count < 2 {
        return None;
    }
    Some(BoundedArrayAlternativeShape {
        prefix: terms[..first_array].to_vec(),
        needle,
        global,
        count,
    })
}

fn array_equality(expression: &Expression) -> Option<(&str, &str, usize)> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    fn pair<'a>(
        needle: &'a Expression,
        element: &'a Expression,
    ) -> Option<(&'a str, &'a str, usize)> {
        let Expression::Variable(needle) = needle else {
            return None;
        };
        let Expression::Index { base, index } = element else {
            return None;
        };
        let Expression::Variable(global) = base.as_ref() else {
            return None;
        };
        let index = usize::try_from(constant_value(index)?).ok()?;
        Some((needle, global, index))
    }
    pair(left, right).or_else(|| pair(right, left))
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

    fn array_equal(needle: &str, global: &str, index: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::Variable(needle.into())),
            right: Box::new(Expression::Index {
                base: Box::new(Expression::Variable(global.into())),
                index: Box::new(Expression::IntegerLiteral(index)),
            }),
        }
    }

    #[test]
    fn recognizes_an_ordered_bounded_array_suffix() {
        let expression = or(
            or(
                Expression::Variable("dense_prefix".into()),
                array_equal("command", "commands", 0),
            ),
            array_equal("command", "commands", 1),
        );
        let shape =
            bounded_array_alternative_shape(&expression).expect("bounded array alternatives");
        assert_eq!(shape.prefix.len(), 1);
        assert_eq!(shape.needle, "command");
        assert_eq!(shape.global, "commands");
        assert_eq!(shape.count, 2);
    }

    #[test]
    fn rejects_a_gapped_or_mixed_array_suffix() {
        let gapped = or(
            or(
                Expression::Variable("dense_prefix".into()),
                array_equal("command", "commands", 0),
            ),
            array_equal("command", "commands", 2),
        );
        assert!(bounded_array_alternative_shape(&gapped).is_none());

        let mixed = or(
            or(
                Expression::Variable("dense_prefix".into()),
                array_equal("command", "commands", 0),
            ),
            array_equal("command", "other", 1),
        );
        assert!(bounded_array_alternative_shape(&mixed).is_none());
    }
}
