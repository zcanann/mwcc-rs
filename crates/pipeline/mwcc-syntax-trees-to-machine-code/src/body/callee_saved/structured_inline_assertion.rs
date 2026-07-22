//! Side effects retained from expression-valued inline helpers.
//!
//! Header helpers commonly begin with `p ? (void)0 : __assert(...)` and are
//! called on the right of `p != NULL && ...`. Build 163 keeps the assertion even
//! though the preceding term proves `p` non-null, reusing that term's CR0 result
//! for the branch around the cold call. This module recognizes that provenance
//! shape and emits the retained side effect without repeating the comparison.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn emit_proven_inline_assertion(
        &mut self,
        previous_term: &Expression,
        term: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let (negated, comma) = match term {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } => (true, operand.as_ref()),
            _ => (false, term),
        };
        let Expression::Comma { left, right } = comma else {
            return Ok(None);
        };
        let Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } = left.as_ref()
        else {
            return Ok(None);
        };
        let Expression::Variable(asserted_name) = condition.as_ref() else {
            return Ok(None);
        };
        if proven_nonzero_name(previous_term) != Some(asserted_name.as_str())
            || !is_void_noop(when_true)
        {
            return Ok(None);
        }
        let Expression::Call { name, arguments } = when_false.as_ref() else {
            return Ok(None);
        };
        if name != "__assert" {
            return Ok(None);
        }

        let assertion_end = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, assertion_end);
        self.emit_call(name, arguments, None, false)?;
        self.bind_label(assertion_end);

        if negated {
            if let Some(condition) = self.try_emit_inlined_boolean_result(right)? {
                return Ok(Some(condition));
            }
        }
        let remainder = if negated {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: right.clone(),
            }
        } else {
            right.as_ref().clone()
        };
        self.emit_condition_test(&remainder).map(Some)
    }

    fn try_emit_inlined_boolean_result(
        &mut self,
        expression: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some((member, first_mask, second_mask)) = shared_member_mask_conjunction(expression)
        else {
            return Ok(None);
        };
        let Some((first_begin, first_end)) = mask_to_run(first_mask) else {
            return Ok(None);
        };
        let Some((second_begin, second_end)) = mask_to_run(second_mask) else {
            return Ok(None);
        };

        let flags = self.fresh_virtual_general_preferring(4);
        let result = self.fresh_virtual_general_preferring(Eabi::general_result().number);
        self.evaluate_general(member, flags)?;
        self.load_integer_constant(result, 0);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: GENERAL_SCRATCH,
            s: flags,
            begin: first_begin,
            end: first_end,
        });
        let result_ready = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, result_ready);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: GENERAL_SCRATCH,
            s: flags,
            begin: second_begin,
            end: second_end,
        });
        self.emit_branch_conditional_to(12, 2, result_ready);
        self.load_integer_constant(result, 1);
        self.bind_label(result_ready);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: result,
                immediate: 0,
            });
        Ok(Some((4, 2)))
    }
}

fn shared_member_mask_conjunction(expression: &Expression) -> Option<(&Expression, u32, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand: first,
    } = left.as_ref()
    else {
        return None;
    };
    let (first_member, first_mask) = member_mask(first)?;
    let (second_member, second_mask) = member_mask(right)?;
    same_member(first_member, second_member)
        .then_some((first_member, first_mask, second_mask))
}

fn member_mask(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let mask = constant_value(right).and_then(|value| u32::try_from(value).ok())?;
    matches!(left.as_ref(), Expression::Member { .. }).then_some((left, mask))
}

fn same_member(left: &Expression, right: &Expression) -> bool {
    matches!((left, right), (
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
        && matches!((left_base.as_ref(), right_base.as_ref()),
            (Expression::Variable(left), Expression::Variable(right)) if left == right))
}

fn proven_nonzero_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => match left.as_ref() {
            Expression::Variable(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

fn is_void_noop(expression: &Expression) -> bool {
    matches!(expression, Expression::Cast {
        target_type: Type::Void,
        operand,
    } if matches!(operand.as_ref(), Expression::IntegerLiteral(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_explicit_nonzero_comparison() {
        let comparison = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable("object".into())),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        assert_eq!(proven_nonzero_name(&comparison), Some("object"));
    }
}
