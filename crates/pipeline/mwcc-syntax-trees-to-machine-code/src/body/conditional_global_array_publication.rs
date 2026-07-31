//! Conditional publication of three incoming words through a global array.
//!
//! Optimized mwcc treats this as one transaction: the first store updates the
//! absolute array base, the publication flag is written in its latency window,
//! and the remaining words reuse that base.  Lowering each source store in
//! isolation rematerializes the array address and loses that schedule.

#[allow(unused_imports)]
use super::*;

struct Publication<'a> {
    condition: &'a str,
    array: &'a str,
    flag: &'a str,
    fallback: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn indexed_parameter_store<'a>(
    statement: &'a Statement,
    expected_index: i64,
    expected_parameter: &str,
) -> Option<&'a str> {
    let Statement::Store {
        target: Expression::Index { base, index },
        value,
    } = statement
    else {
        return None;
    };
    let Expression::Variable(array) = base.as_ref() else {
        return None;
    };
    (constant_value(index) == Some(expected_index) && variable(value, expected_parameter))
        .then_some(array)
}

fn constant_global_store(statement: &Statement, expected: i64) -> Option<&str> {
    let Statement::Store {
        target: Expression::Variable(global),
        value,
    } = statement
    else {
        return None;
    };
    (constant_value(value) == Some(expected)).then_some(global)
}

fn classify_if<'a>(statement: &'a Statement, parameters: [&str; 3]) -> Option<Publication<'a>> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let Expression::Variable(condition) = left.as_ref() else {
        return None;
    };
    if constant_value(right) != Some(1) {
        return None;
    }
    let [first, second, third, publish] = then_body.as_slice() else {
        return None;
    };
    let array = indexed_parameter_store(first, 0, parameters[0])?;
    if indexed_parameter_store(second, 1, parameters[1])? != array
        || indexed_parameter_store(third, 2, parameters[2])? != array
    {
        return None;
    }
    let flag = constant_global_store(publish, 1)?;
    let [Statement::Expression(Expression::Call {
        name: fallback,
        arguments,
    }), clear] = else_body.as_slice()
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [first, second, third]
        if variable(first, parameters[0])
            && variable(second, parameters[1])
            && variable(third, parameters[2]))
        || constant_global_store(clear, 0)? != flag
    {
        return None;
    }
    Some(Publication {
        condition,
        array,
        flag,
        fallback,
    })
}

impl Generator {
    pub(crate) fn try_conditional_global_array_publication(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.asm_body.is_some()
            || !function.inline_asm_blocks.is_empty()
        {
            return Ok(false);
        }
        let [first, second, third] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if ![first, second, third]
            .into_iter()
            .all(|parameter| matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        {
            return Ok(false);
        }
        let [statement] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some(publication) = classify_if(statement, [&first.name, &second.name, &third.name])
        else {
            return Ok(false);
        };
        if self.lookup_general(&first.name) != Some(3)
            || self.lookup_general(&second.name) != Some(4)
            || self.lookup_general(&third.name) != Some(5)
            || self.globals.get(publication.condition) != Some(&Type::UnsignedChar)
            || !matches!(
                self.globals.get(publication.array),
                Some(Type::Int | Type::UnsignedInt)
            )
            || !matches!(
                self.globals.get(publication.flag),
                Some(Type::Int | Type::UnsignedInt)
            )
            || self.global_array_sizes.get(publication.array) != Some(&12)
        {
            return Ok(false);
        }

        self.emit_plain_nonleaf_prologue();
        self.emit_global_load_value(publication.condition, GENERAL_SCRATCH)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: GENERAL_SCRATCH,
                immediate: 1,
            });
        let fallback = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, fallback);

        self.emit_address_high(6, publication.array);
        self.record_relocation(RelocationKind::Addr16Lo, publication.array);
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 3,
                a: 6,
                offset: 0,
            });
        self.load_integer_constant(GENERAL_SCRATCH, 1);
        self.emit_global_store(publication.flag, Pointee::UnsignedInt, GENERAL_SCRATCH)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 6,
            offset: 8,
        });
        self.emit_branch_to(join);

        self.bind_label(fallback);
        self.record_relocation(RelocationKind::Rel24, publication.fallback);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: publication.fallback.to_string(),
        });
        self.load_integer_constant(GENERAL_SCRATCH, 0);
        self.emit_global_store(publication.flag, Pointee::UnsignedInt, GENERAL_SCRATCH)?;

        self.bind_label(join);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(array: &str, index: i64, parameter: &str) -> Statement {
        Statement::Store {
            target: Expression::Index {
                base: Box::new(Expression::Variable(array.into())),
                index: Box::new(Expression::IntegerLiteral(index)),
            },
            value: Expression::Variable(parameter.into()),
        }
    }

    fn candidate() -> Statement {
        Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(Expression::Variable("prior_yield".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            },
            then_body: vec![
                store("words", 0, "a"),
                store("words", 1, "b"),
                store("words", 2, "c"),
                Statement::Store {
                    target: Expression::Variable("pending".into()),
                    value: Expression::IntegerLiteral(1),
                },
            ],
            else_body: vec![
                Statement::Expression(Expression::Call {
                    name: "fallback".into(),
                    arguments: ["a", "b", "c"]
                        .into_iter()
                        .map(|name| Expression::Variable(name.into()))
                        .collect(),
                }),
                Statement::Store {
                    target: Expression::Variable("pending".into()),
                    value: Expression::IntegerLiteral(0),
                },
            ],
        }
    }

    #[test]
    fn recognizes_the_complete_publication_transaction() {
        let statement = candidate();
        let shape = classify_if(&statement, ["a", "b", "c"]).expect("publication");
        assert_eq!(shape.condition, "prior_yield");
        assert_eq!(shape.array, "words");
        assert_eq!(shape.flag, "pending");
        assert_eq!(shape.fallback, "fallback");
    }

    #[test]
    fn rejects_a_different_fallback_argument_order() {
        let mut statement = candidate();
        let Statement::If { else_body, .. } = &mut statement else {
            unreachable!()
        };
        let Statement::Expression(Expression::Call { arguments, .. }) = &mut else_body[0] else {
            unreachable!()
        };
        arguments.swap(0, 1);
        assert!(classify_if(&statement, ["a", "b", "c"]).is_none());
    }
}
