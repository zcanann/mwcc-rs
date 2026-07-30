//! Guarded indirect calls through repeated indexed global-table entries.
//!
//! The condition and call name the same source expression, but MWCC evaluates
//! the entry once and carries it down the true edge in r12.

use super::*;

impl Generator {
    pub(crate) fn is_guarded_indexed_indirect_call(
        &self,
        condition: &Expression,
        then_body: &[Statement],
    ) -> bool {
        recognize(condition, then_body, &self.globals).is_some()
    }

    pub(crate) fn try_emit_guarded_indexed_indirect_call(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
    ) -> Compilation<bool> {
        let Some((entry, arguments)) = recognize(condition, then_body, &self.globals) else {
            return Ok(false);
        };
        let placements = self.indirect_argument_placements(arguments)?;

        self.evaluate(entry, Type::UnsignedInt, 12)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done);
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.const_address_bases.clear();
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            for placement in &placements {
                match *placement {
                    ArgumentPlacement::Register { source, target } if source != target => {
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: target,
                            a: source,
                            immediate: 0,
                        });
                    }
                    ArgumentPlacement::Constant { value, target } => {
                        self.load_integer_constant(target, value);
                    }
                    ArgumentPlacement::Register { .. } => {}
                }
            }
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
        } else {
            self.emit_indirect_arguments(arguments, &placements)?;
            self.emit_indirect_branch_and_link(12);
        }
        self.bind_label(done);
        Ok(true)
    }
}

fn recognize<'a>(
    condition: &'a Expression,
    then_body: &'a [Statement],
    globals: &std::collections::HashMap<String, Type>,
) -> Option<(&'a Expression, &'a [Expression])> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let [Statement::Expression(Expression::CallThrough { target, arguments })] = then_body else {
        return None;
    };
    same_indexed_global_entry(left, target, globals)
        .then_some((left.as_ref(), arguments.as_slice()))
}

fn same_indexed_global_entry(
    left: &Expression,
    right: &Expression,
    globals: &std::collections::HashMap<String, Type>,
) -> bool {
    let (
        Expression::Index {
            base: left_base,
            index: left_index,
        },
        Expression::Index {
            base: right_base,
            index: right_index,
        },
    ) = (left, right)
    else {
        return false;
    };
    let (Expression::Variable(left_global), Expression::Variable(right_global)) =
        (left_base.as_ref(), right_base.as_ref())
    else {
        return false;
    };
    left_global == right_global
        && globals.contains_key(left_global)
        && same_member_index(left_index, right_index)
}

fn same_member_index(left: &Expression, right: &Expression) -> bool {
    matches!(
        (left, right),
        (
            Expression::Member {
                base: left_base,
                offset: left_offset,
                member_type: left_type,
                index_stride: left_stride,
            },
            Expression::Member {
                base: right_base,
                offset: right_offset,
                member_type: right_type,
                index_stride: right_stride,
            },
        ) if left_offset == right_offset
            && left_type == right_type
            && left_stride == right_stride
            && matches!(
                (left_base.as_ref(), right_base.as_ref()),
                (Expression::Variable(left_name), Expression::Variable(right_name))
                    if left_name == right_name
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(table: &str, offset: u32) -> Expression {
        Expression::Index {
            base: Box::new(Expression::Variable(table.into())),
            index: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset,
                member_type: Type::Int,
                index_stride: None,
            }),
        }
    }

    #[test]
    fn recognizes_a_repeated_indexed_global_callback_entry() {
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(entry("callbacks", 4)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let body = vec![Statement::Expression(Expression::CallThrough {
            target: Box::new(entry("callbacks", 4)),
            arguments: vec![Expression::Variable("object".into())],
        })];
        let globals = std::collections::HashMap::from([(
            "callbacks".into(),
            Type::Pointer(Pointee::UnsignedInt),
        )]);

        assert!(recognize(&condition, &body, &globals).is_some());
    }

    #[test]
    fn rejects_a_different_callback_entry_in_the_body() {
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(entry("callbacks", 4)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let body = vec![Statement::Expression(Expression::CallThrough {
            target: Box::new(entry("callbacks", 8)),
            arguments: Vec::new(),
        })];
        let globals = std::collections::HashMap::from([(
            "callbacks".into(),
            Type::Pointer(Pointee::UnsignedInt),
        )]);

        assert!(recognize(&condition, &body, &globals).is_none());
    }
}
