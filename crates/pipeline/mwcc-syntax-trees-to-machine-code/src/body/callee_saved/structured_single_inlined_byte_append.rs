//! Ownership for one value-returning inlined byte-buffer append.
//!
//! Unlike repeated side-effect-only appends, this form preserves both error
//! results. MWCC keeps the guarded cursor live across the success edge, then
//! reuses its scratch and address registers while publishing the zero result.

#[allow(unused_imports)]
use super::*;

pub(super) fn has_single_value_inlined_byte_append(function: &Function) -> bool {
    count_value_inlined_byte_appends(&function.statements) == 1
}

fn count_value_inlined_byte_appends(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                usize::from(is_value_inlined_byte_append(
                    condition,
                    then_body,
                    else_body,
                )) + count_value_inlined_byte_appends(then_body)
                    + count_value_inlined_byte_appends(else_body)
            }
            _ => 0,
        })
        .sum()
}

fn is_value_inlined_byte_append(
    condition: &Expression,
    then_body: &[Statement],
    else_body: &[Statement],
) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left,
        right: capacity,
    } = condition
    else {
        return false;
    };
    let Expression::Member {
        base: guard_base,
        offset: cursor_offset,
        ..
    } = left.as_ref()
    else {
        return false;
    };
    let (Expression::Variable(buffer), Expression::IntegerLiteral(_)) =
        (guard_base.as_ref(), capacity.as_ref())
    else {
        return false;
    };
    let (
        [Statement::Assign {
            name: error_name,
            value: Expression::IntegerLiteral(_),
        }],
        [
            Statement::Store {
                target: byte_target,
                ..
            },
            Statement::Store {
                target: length_target,
                value: length_value,
            },
            Statement::Assign {
                name: success_name,
                value: Expression::IntegerLiteral(0),
            },
        ],
    ) = (then_body, else_body)
    else {
        return false;
    };
    if error_name != success_name {
        return false;
    }
    let Expression::Index {
        base: byte_base,
        index,
    } = byte_target
    else {
        return false;
    };
    let Expression::MemberAddress {
        base: byte_buffer,
        ..
    } = byte_base.as_ref()
    else {
        return false;
    };
    let Expression::PostStep {
        target: stepped_cursor,
        operator: BinaryOperator::Add,
        ..
    } = index.as_ref()
    else {
        return false;
    };
    let Expression::Member {
        base: cursor_buffer,
        offset: stepped_offset,
        ..
    } = stepped_cursor.as_ref()
    else {
        return false;
    };
    let Expression::Member {
        base: length_buffer,
        offset: length_offset,
        ..
    } = length_target
    else {
        return false;
    };
    let Expression::IndexedUpdateValue { value: length_value } = length_value else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_length,
        right: increment,
    } = length_value.as_ref()
    else {
        return false;
    };
    let Expression::Member {
        base: old_length_buffer,
        offset: old_length_offset,
        ..
    } = old_length.as_ref()
    else {
        return false;
    };
    matches!(
        (
            byte_buffer.as_ref(),
            cursor_buffer.as_ref(),
            length_buffer.as_ref(),
            old_length_buffer.as_ref(),
            increment.as_ref(),
        ),
        (
            Expression::Variable(byte),
            Expression::Variable(cursor),
            Expression::Variable(length),
            Expression::Variable(old_length),
            Expression::IntegerLiteral(1),
        ) if byte == buffer
            && cursor == buffer
            && length == buffer
            && old_length == buffer
            && stepped_offset == cursor_offset
            && old_length_offset == length_offset
    )
}

impl Generator {
    /// The owner passed beside a frame-array address is a final value
    /// materialization in build 163, not the generic pointer-preservation copy.
    pub(super) fn schedule_single_inlined_byte_append_owner_argument(&mut self, source: u8) {
        if self.behavior.materialization_copy_style
            != mwcc_versions::MaterializationCopyStyle::AddImmediateZero
        {
            return;
        }
        let Some(index) = self.output.instructions.windows(4).rposition(|window| {
            matches!(window[0], Instruction::Or { a: 3, s, b } if s == source && b == source)
                && matches!(window[1], Instruction::AddImmediate { d: 4, a: 1, .. })
                && matches!(window[2], Instruction::AddImmediate { d: 5, a: 0, .. })
                && matches!(window[3], Instruction::BranchAndLink { .. })
        }) else {
            return;
        };
        self.output.instructions[index] = Instruction::AddImmediate {
            d: 3,
            a: source,
            immediate: 0,
        };
    }

    pub(crate) fn schedule_structured_single_inlined_byte_append(&mut self) {
        let Some(plan) = value_inlined_byte_append(&self.output.instructions) else {
            return;
        };
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.start] else {
            unreachable!("the append cursor load was matched")
        };
        *d = plan.cursor;
        let Instruction::CompareLogicalWordImmediate { a, .. } =
            &mut self.output.instructions[plan.start + 1]
        else {
            unreachable!("the append cursor comparison was matched")
        };
        *a = plan.cursor;

        crate::remove_instruction_retargeting_to_next(self, plan.start + 5);
        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.start + 5]
        else {
            unreachable!("the append cursor increment was matched")
        };
        *d = plan.scratch;
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[plan.start + 6]
        else {
            unreachable!("the append cursor publication was matched")
        };
        *s = plan.scratch;

        crate::move_instruction_before_retargeting(self, plan.start + 8, plan.start + 6);
        crate::move_instruction_before_retargeting(self, plan.start + 13, plan.start + 9);

        let Instruction::LoadWord { d, .. } =
            &mut self.output.instructions[plan.start + 11]
        else {
            unreachable!("the append length load was matched")
        };
        *d = plan.byte_address;
        let Instruction::AddImmediate { d, a, .. } =
            &mut self.output.instructions[plan.start + 12]
        else {
            unreachable!("the append length increment was matched")
        };
        *d = plan.scratch;
        *a = plan.byte_address;
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[plan.start + 13]
        else {
            unreachable!("the append length publication was matched")
        };
        *s = plan.scratch;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ValueInlineByteAppend {
    start: usize,
    cursor: u8,
    scratch: u8,
    byte_address: u8,
}

fn value_inlined_byte_append(instructions: &[Instruction]) -> Option<ValueInlineByteAppend> {
    instructions.windows(15).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: guarded_cursor, a: guard_buffer, offset: guard_offset },
            Instruction::CompareLogicalWordImmediate { a: guarded, .. },
            Instruction::BranchConditionalForward { options, condition_bit: 0, target: success },
            Instruction::AddImmediate { d: error_result, a: 0, .. },
            Instruction::Branch { target: end },
            Instruction::LoadWord { d: cursor, a: cursor_buffer, offset: cursor_offset },
            Instruction::AddImmediate { d: incremented_cursor, a: incremented_from, immediate: 1 },
            Instruction::StoreWord { s: stored_cursor, a: cursor_store_buffer, offset: stored_cursor_offset },
            Instruction::AddImmediate { d: byte, a: 0, .. },
            Instruction::Add { d: byte_address, a: append_buffer, b: cursor_index },
            Instruction::StoreByte { s: stored_byte, a: byte_base, .. },
            Instruction::LoadWord { d: old_length, a: length_buffer, offset: length_offset },
            Instruction::AddImmediate { d: new_length, a: incremented_length, immediate: 1 },
            Instruction::StoreWord { s: stored_length, a: length_store_buffer, offset: stored_length_offset },
            Instruction::AddImmediate { d: success_result, a: 0, immediate: 0 },
        ] = window else {
            return None;
        };
        (*options == 12
            && *success == start + 5
            && *end == start + 15
            && *error_result == Eabi::general_result().number
            && *success_result == *error_result
            && *guarded == *guarded_cursor
            && *guarded_cursor == 0
            && *guard_buffer == *cursor_buffer
            && *guard_buffer == *cursor_store_buffer
            && *guard_buffer == *append_buffer
            && *guard_buffer == *length_buffer
            && *guard_buffer == *length_store_buffer
            && *guard_offset == *cursor_offset
            && *guard_offset == *stored_cursor_offset
            && *incremented_from == *cursor
            && *stored_cursor == *incremented_cursor
            && *cursor_index == *cursor
            && *byte == *guarded_cursor
            && *stored_byte == *byte
            && *byte_base == *byte_address
            && *incremented_cursor == *byte_address
            && *incremented_length == *old_length
            && *stored_length == *new_length
            && *length_offset == *stored_length_offset
            && *cursor != 0
            && *cursor != 1)
            .then_some(ValueInlineByteAppend {
                start,
                cursor: *cursor,
                scratch: *guarded_cursor,
                byte_address: *byte_address,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("buffer".to_owned())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_the_expanded_value_append_shape() {
        let condition = Expression::Binary {
            operator: BinaryOperator::GreaterEqual,
            left: Box::new(buffer_member(12)),
            right: Box::new(Expression::IntegerLiteral(2176)),
        };
        let then_body = vec![Statement::Assign {
            name: "error".to_owned(),
            value: Expression::IntegerLiteral(769),
        }];
        let else_body = vec![
            Statement::Store {
                target: Expression::Index {
                    base: Box::new(Expression::MemberAddress {
                        base: Box::new(Expression::Variable("buffer".to_owned())),
                        offset: 16,
                        element: mwcc_syntax_trees::Pointee::UnsignedChar,
                        index_stride: None,
                    }),
                    index: Box::new(Expression::PostStep {
                        target: Box::new(buffer_member(12)),
                        operator: BinaryOperator::Add,
                        pointer_link: None,
                    }),
                },
                value: Expression::IntegerLiteral(2),
            },
            Statement::Store {
                target: buffer_member(8),
                value: Expression::IndexedUpdateValue {
                    value: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(buffer_member(8)),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    }),
                },
            },
            Statement::Assign {
                name: "error".to_owned(),
                value: Expression::IntegerLiteral(0),
            },
        ];

        assert!(is_value_inlined_byte_append(
            &condition,
            &then_body,
            &else_body,
        ));
    }

    #[test]
    fn recognizes_a_value_returning_append_window() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 31, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2176 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 5 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 769 },
            Instruction::Branch { target: 15 },
            Instruction::LoadWord { d: 3, a: 31, offset: 12 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 4, a: 31, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 2 },
            Instruction::Add { d: 4, a: 31, b: 3 },
            Instruction::StoreByte { s: 0, a: 4, offset: 16 },
            Instruction::LoadWord { d: 3, a: 31, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 31, offset: 8 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        ];

        assert_eq!(
            value_inlined_byte_append(&instructions),
            Some(ValueInlineByteAppend {
                start: 0,
                cursor: 3,
                scratch: 0,
                byte_address: 4,
            })
        );
    }
}
