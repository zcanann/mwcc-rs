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
    /// Fold a cold assertion's inverted two-branch diamond and schedule the
    /// following float-member return. The incoming object remains available in
    /// r3 until the cold call, while its saved copy owns the post-call return.
    pub(crate) fn schedule_assertion_float_member_return(&mut self) {
        let Some((start, has_redundant_branch)) =
            (0..self.output.instructions.len()).find_map(|start| {
                let remaining = &self.output.instructions[start..];
                if remaining
                    .get(..18)
                    .is_some_and(is_unfolded_assertion_float_member_return)
                {
                    Some((start, true))
                } else if remaining
                    .get(..17)
                    .is_some_and(is_assertion_float_member_return)
                {
                    Some((start, false))
                } else {
                    None
                }
            })
        else {
            return;
        };
        let (saved, entry) = match self.output.instructions[start] {
            Instruction::Or { a: saved, s: entry, b } if entry == b => (saved, entry),
            _ => unreachable!(),
        };
        match &mut self.output.instructions[start + 1] {
            Instruction::LoadWord { a, .. } => *a = entry,
            _ => unreachable!(),
        }
        if has_redundant_branch {
            let join = match self.output.instructions[start + 4] {
                Instruction::Branch { target } => target,
                _ => unreachable!(),
            };
            self.output.instructions[start + 3] = Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: join,
            };
            self.remove_structured_condition_instruction(start + 4);
        }
        // MWCC starts restoring LR between the dependent pointer and float
        // loads, then completes the float load before restoring the saved GPR.
        self.output.instructions.swap(start + 11, start + 12);
        debug_assert!(matches!(
            self.output.instructions[start + 13],
            Instruction::LoadWord { d, .. } if d == saved
        ));
    }

    pub(super) fn emit_proven_inline_assertion(
        &mut self,
        previous_term: &Expression,
        term: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(parts) = inline_assertion_parts(term) else {
            return Ok(None);
        };
        if proven_nonzero_name(previous_term) != Some(parts.asserted_name) {
            return Ok(None);
        }

        let assertion_end = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, assertion_end);
        self.emit_call("__assert", parts.arguments, None, false)?;
        self.bind_label(assertion_end);

        if let Some(condition) = self.try_emit_inlined_boolean_result(parts.remainder)? {
            return Ok(Some(if parts.negated {
                condition
            } else {
                (condition.0 ^ 8, condition.1)
            }));
        }
        let remainder = if parts.negated {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: Box::new(parts.remainder.clone()),
            }
        } else {
            parts.remainder.clone()
        };
        self.emit_condition_test(&remainder).map(Some)
    }

    /// Emit an expression-valued inline assertion when it is itself the first
    /// condition term. Unlike the proven-nonzero form above, this owns the
    /// pointer test that skips the cold assertion before lowering the retained
    /// boolean result.
    pub(super) fn emit_leading_inline_assertion(
        &mut self,
        term: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(parts) = leading_inline_assertion_parts(term) else {
            return Ok(None);
        };
        let (options, condition_bit) = self.emit_condition_test(parts.condition)?;
        let assertion_end = self.fresh_label();
        self.emit_branch_conditional_to(options ^ 8, condition_bit, assertion_end);
        self.emit_call("__assert", parts.arguments, None, false)?;
        self.bind_label(assertion_end);

        if let Some(condition) = self.try_emit_inlined_boolean_result(parts.remainder)? {
            return Ok(Some(if parts.negated {
                condition
            } else {
                (condition.0 ^ 8, condition.1)
            }));
        }
        let remainder = if parts.negated {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: Box::new(parts.remainder.clone()),
            }
        } else {
            parts.remainder.clone()
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

struct InlineAssertionParts<'a> {
    negated: bool,
    condition: &'a Expression,
    asserted_name: &'a str,
    arguments: &'a [Expression],
    remainder: &'a Expression,
}

fn inline_assertion_parts(term: &Expression) -> Option<InlineAssertionParts<'_>> {
    let (negated, comma) = match term {
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => (true, operand.as_ref()),
        _ => (false, term),
    };
    let Expression::Comma { left, right } = comma else {
        return None;
    };
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Variable(asserted_name) = condition.as_ref() else {
        return None;
    };
    let Expression::Call { name, arguments } = when_false.as_ref() else {
        return None;
    };
    (is_void_noop(when_true) && name == "__assert").then_some(InlineAssertionParts {
        negated,
        condition,
        asserted_name,
        arguments,
        remainder: right,
    })
}

fn leading_inline_assertion_parts(term: &Expression) -> Option<InlineAssertionParts<'_>> {
    let parts = inline_assertion_parts(term)?;
    let (_, first_mask, second_mask) = shared_member_mask_conjunction(parts.remainder)?;
    (mask_to_run(first_mask).is_some() && mask_to_run(second_mask).is_some()).then_some(parts)
}

fn is_assertion_float_member_return(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: saved, s: entry, b: entry_again },
        Instruction::LoadWord { d: 0, a: condition_base, .. },
        Instruction::CompareWordImmediate { a: 0, .. },
        Instruction::BranchConditionalForward { options: 12, condition_bit: 2, .. },
        _, _, _, _, _,
        Instruction::BranchAndLink { target },
        Instruction::LoadWord { d: 3, a: return_base, .. },
        Instruction::LoadFloatSingle { d: 1, a: 3, .. },
        Instruction::LoadWord { d: 0, a: 1, .. },
        Instruction::LoadWord { d: restored, a: 1, .. },
        Instruction::AddImmediate { d: 1, a: 1, .. },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ] if saved != entry
        && entry == entry_again
        && condition_base == saved
        && return_base == saved
        && restored == saved
        && target == "__assert")
}

fn is_unfolded_assertion_float_member_return(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: saved, s: entry, b: entry_again },
        Instruction::LoadWord { d: 0, a: condition_base, .. },
        Instruction::CompareWordImmediate { a: 0, .. },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 2, .. },
        Instruction::Branch { .. },
        _, _, _, _, _,
        Instruction::BranchAndLink { target },
        Instruction::LoadWord { d: 3, a: return_base, .. },
        Instruction::LoadFloatSingle { d: 1, a: 3, .. },
        Instruction::LoadWord { d: 0, a: 1, .. },
        Instruction::LoadWord { d: restored, a: 1, .. },
        Instruction::AddImmediate { d: 1, a: 1, .. },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ] if saved != entry
        && entry == entry_again
        && condition_base == saved
        && return_base == saved
        && restored == saved
        && target == "__assert")
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

    fn masked_member(mask: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset: 20,
                member_type: Type::UnsignedInt,
                index_stride: None,
            }),
            right: Box::new(Expression::IntegerLiteral(mask)),
        }
    }

    fn leading_assertion_with_remainder(remainder: Expression) -> Expression {
        Expression::Comma {
            left: Box::new(Expression::Conditional {
                condition: Box::new(Expression::Variable("object".into())),
                when_true: Box::new(Expression::Cast {
                    target_type: Type::Void,
                    operand: Box::new(Expression::IntegerLiteral(0)),
                }),
                when_false: Box::new(Expression::Call {
                    name: "__assert".into(),
                    arguments: Vec::new(),
                }),
                origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
            }),
            right: Box::new(remainder),
        }
    }

    #[test]
    fn recognizes_a_leading_assertion_with_shared_member_masks() {
        let remainder = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: Box::new(masked_member(0x0080_0000)),
            }),
            right: Box::new(masked_member(0x40)),
        };
        let expression = leading_assertion_with_remainder(remainder);
        let parts = leading_inline_assertion_parts(&expression)
            .expect("the inline assertion and shared masks should be recognized");
        assert_eq!(parts.asserted_name, "object");
    }

    #[test]
    fn rejects_a_leading_assertion_without_the_shared_mask_provenance() {
        let expression = leading_assertion_with_remainder(Expression::Variable("flag".into()));
        assert!(leading_inline_assertion_parts(&expression).is_none());
    }

    #[test]
    fn recognizes_explicit_nonzero_comparison() {
        let comparison = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable("object".into())),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        assert_eq!(proven_nonzero_name(&comparison), Some("object"));
    }

    #[test]
    fn recognizes_a_cold_assertion_before_a_float_member_return() {
        let instructions = [
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::LoadWord { d: 0, a: 31, offset: 4 },
            Instruction::CompareWordImmediate { a: 0, immediate: 32 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 10 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 299 },
            Instruction::BranchAndLink { target: "__assert".into() },
            Instruction::LoadWord { d: 3, a: 31, offset: 724 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        assert!(is_assertion_float_member_return(&instructions));
    }

    #[test]
    fn recognizes_an_unfolded_cold_assertion_before_a_float_member_return() {
        let mut instructions = vec![
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::LoadWord { d: 0, a: 31, offset: 4 },
            Instruction::CompareWordImmediate { a: 0, immediate: 32 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 5 },
            Instruction::Branch { target: 11 },
        ];
        instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 299 },
            Instruction::BranchAndLink { target: "__assert".into() },
            Instruction::LoadWord { d: 3, a: 31, offset: 724 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        assert!(is_unfolded_assertion_float_member_return(&instructions));
    }
}
