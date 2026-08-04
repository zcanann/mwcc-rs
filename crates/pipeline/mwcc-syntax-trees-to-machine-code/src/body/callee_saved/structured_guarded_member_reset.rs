//! Shared-zero member reset followed by a guarded pointer cleanup.
//!
//! Optimized teardown code commonly resets adjacent scalar state, tests an
//! owned pointer, calls its cleanup routine, and clears that pointer. MWCC keeps
//! both the owner and the named zero in callee-saved homes, reuses the pointer
//! test load as the call argument, and reuses the saved zero after the call.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};

pub(super) struct StructuredGuardedMemberReset {
    owner: String,
    zero: String,
    pointer_offset: i16,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct StructuredGuardedMemberResetHomes {
    owner: u8,
    zero: u8,
}

impl StructuredGuardedMemberReset {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        if function.return_type != Type::Void
            || function.parameters.len() != 1
            || function.statements.len() != 1
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return None;
        }
        let parameter = &function.parameters[0];
        let (owner, zero) = match function.locals.as_slice() {
            [zero] => (parameter.name.as_str(), zero),
            [owner, zero]
                if matches!(owner.declared_type, Type::StructPointer { .. })
                    && matches!(
                        owner.initializer,
                        Some(Expression::Variable(ref name)) if name == &parameter.name
                    ) => (owner.name.as_str(), zero),
            _ => return None,
        };
        if zero.declared_type != Type::Int
            || zero.initializer.is_some()
        {
            return None;
        }
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = &function.statements[0]
        else {
            return None;
        };
        if !else_body.is_empty()
            || !member_nonzero(condition, owner, 0)
            || then_body.len() < 7
        {
            return None;
        }
        let reset_start = then_body.len() - 4;
        let [assign_zero, first_reset, second_reset, guarded_cleanup] =
            &then_body[reset_start..]
        else {
            return None;
        };
        if !matches!(
            assign_zero,
            Statement::Assign {
                name,
                value: Expression::IntegerLiteral(0),
            } if name == &zero.name
        ) || !member_store_from(first_reset, owner, 0, &zero.name)
            || !member_store_from(second_reset, owner, 1, &zero.name)
            || !then_body[..reset_start].iter().all(call_statement)
        {
            return None;
        }
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = guarded_cleanup
        else {
            return None;
        };
        let [cleanup_call, clear_pointer] = then_body.as_slice() else {
            return None;
        };
        if !else_body.is_empty() {
            return None;
        }
        let pointer_offset = member_nonzero_offset(condition, owner)?;
        if pointer_offset <= 1
            || !call_with_member(cleanup_call, owner, pointer_offset)
            || !member_store_literal(clear_pointer, owner, pointer_offset, 0)
        {
            return None;
        }
        Some(Self {
            owner: owner.to_owned(),
            zero: zero.name.clone(),
            pointer_offset: i16::try_from(pointer_offset).ok()?,
        })
    }

    pub(super) fn names(&self) -> [&str; 2] {
        [&self.owner, &self.zero]
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        if name == self.owner {
            Some(30)
        } else if name == self.zero {
            Some(31)
        } else {
            None
        }
    }

    pub(super) fn homes(
        &self,
        mut home_for: impl FnMut(&str) -> Option<u8>,
    ) -> Option<StructuredGuardedMemberResetHomes> {
        Some(StructuredGuardedMemberResetHomes {
            owner: home_for(&self.owner)?,
            zero: home_for(&self.zero)?,
        })
    }

    pub(super) fn save_order(
        &self,
        homes: StructuredGuardedMemberResetHomes,
    ) -> [u8; 2] {
        [homes.zero, homes.owner]
    }

    pub(super) fn schedule(
        &self,
        generator: &mut Generator,
        homes: StructuredGuardedMemberResetHomes,
    ) {
        let Some(start) = generator.output.instructions.windows(7).position(|window| {
            guarded_clear_window(window, homes.owner, self.pointer_offset)
        }) else {
            return;
        };
        generator.output.instructions[start] = Instruction::LoadWord {
            d: 3,
            a: homes.owner,
            offset: self.pointer_offset,
        };
        generator.output.instructions[start + 1] =
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 };
        generator.output.instructions[start + 6] = Instruction::StoreWord {
            s: homes.zero,
            a: homes.owner,
            offset: self.pointer_offset,
        };
        crate::remove_instruction_retargeting_to_next(generator, start + 5);
        crate::remove_instruction_retargeting_to_next(generator, start + 3);
    }
}

fn member_nonzero(expression: &Expression, owner: &str, offset: u32) -> bool {
    member_nonzero_offset(expression, owner) == Some(offset)
}

fn member_nonzero_offset(expression: &Expression, owner: &str) -> Option<u32> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    member_offset(left, owner)
        .filter(|_| matches!(right.as_ref(), Expression::IntegerLiteral(0)))
        .or_else(|| {
            matches!(left.as_ref(), Expression::IntegerLiteral(0))
                .then(|| member_offset(right, owner))
                .flatten()
        })
}

fn member_offset(expression: &Expression, owner: &str) -> Option<u32> {
    match expression {
        Expression::Member {
            base,
            offset,
            index_stride: None,
            ..
        } if matches!(base.as_ref(), Expression::Variable(name) if name == owner) => Some(*offset),
        _ => None,
    }
}

fn call_statement(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Call { .. }))
}

fn member_store_from(statement: &Statement, owner: &str, offset: u32, value: &str) -> bool {
    matches!(
        statement,
        Statement::Store { target, value: Expression::Variable(name) }
            if name == value && member_offset(target, owner) == Some(offset)
    )
}

fn member_store_literal(
    statement: &Statement,
    owner: &str,
    offset: u32,
    literal: i64,
) -> bool {
    matches!(
        statement,
        Statement::Store { target, value: Expression::IntegerLiteral(value) }
            if *value == literal && member_offset(target, owner) == Some(offset)
    )
}

fn call_with_member(statement: &Statement, owner: &str, offset: u32) -> bool {
    let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
        return false;
    };
    matches!(arguments.as_slice(), [argument] if member_offset(argument, owner) == Some(offset))
}

fn guarded_clear_window(instructions: &[Instruction], owner: u8, offset: i16) -> bool {
    matches!(
        instructions,
        [
            Instruction::LoadWord { d: tested, a: first_owner, offset: first_offset },
            Instruction::CompareLogicalWordImmediate { a: compared, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 3, a: second_owner, offset: second_offset },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: scratch, a: 0, immediate: 0 },
            Instruction::StoreWord { s: stored, a: store_owner, offset: store_offset },
        ] if *tested == *compared
            && *first_owner == owner
            && *second_owner == owner
            && *store_owner == owner
            && *first_offset == offset
            && *second_offset == offset
            && *store_offset == offset
            && *scratch == *stored
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_redundant_guarded_pointer_reload_window() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 30, offset: 8 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 7,
            },
            Instruction::LoadWord { d: 3, a: 30, offset: 8 },
            Instruction::BranchAndLink { target: "cleanup".into() },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 30, offset: 8 },
        ];
        assert!(guarded_clear_window(&instructions, 30, 8));
        assert!(!guarded_clear_window(&instructions, 31, 8));
    }
}
