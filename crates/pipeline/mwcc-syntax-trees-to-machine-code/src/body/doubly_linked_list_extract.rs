//! Scheduled extraction from an intrusive doubly linked list.
//!
//! MWCC retains each tested neighbor across its link repair. The ordinary
//! statement path reloads the nested member and loses both the register choice
//! and a load, so this owner recognizes the complete leaf transaction.

#[allow(unused_imports)]
use super::*;

pub(crate) struct DoublyLinkedListExtract<'a> {
    pub(crate) list: &'a str,
    pub(crate) cell: &'a str,
    pub(crate) previous_offset: i16,
    pub(crate) next_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&Expression, i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((base, i16::try_from(*offset).ok()?, *member_type))
}

fn direct_member(expression: &Expression, base: &str) -> Option<(i16, Type)> {
    let (member_base, offset, member_type) = member(expression)?;
    (variable(member_base) == Some(base)).then_some((offset, member_type))
}

fn nested_member(expression: &Expression, root: &str) -> Option<(i16, Type, i16, Type)> {
    let (inner, outer_offset, outer_type) = member(expression)?;
    let (inner_offset, inner_type) = direct_member(inner, root)?;
    Some((inner_offset, inner_type, outer_offset, outer_type))
}

fn pointer_word(member_type: Type) -> bool {
    matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. })
}

pub(crate) fn summarize(function: &Function) -> Option<DoublyLinkedListExtract<'_>> {
    let [list, cell] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        function.return_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        list.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        cell.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
        || variable(function.return_expression.as_ref()?) != Some(&list.name)
    {
        return None;
    }
    let [Statement::If {
        condition: next_condition,
        then_body: next_body,
        else_body: next_else,
    }, Statement::If {
        condition: previous_condition,
        then_body: previous_body,
        else_body: previous_else,
    }, final_store] = function.statements.as_slice()
    else {
        return None;
    };
    if !next_else.is_empty() || !previous_else.is_empty() {
        return None;
    }

    let (next_offset, next_type) = direct_member(next_condition, &cell.name)?;
    let [Statement::Store {
        target: next_previous,
        value: current_previous,
    }] = next_body.as_slice()
    else {
        return None;
    };
    let (next_base_offset, next_base_type, previous_offset, previous_type) =
        nested_member(next_previous, &cell.name)?;
    let (current_previous_offset, current_previous_type) =
        direct_member(current_previous, &cell.name)?;
    if next_base_offset != next_offset
        || previous_offset != current_previous_offset
        || ![
            next_type,
            next_base_type,
            previous_type,
            current_previous_type,
        ]
        .into_iter()
        .all(pointer_word)
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = previous_condition
    else {
        return None;
    };
    let tested_previous = if constant_value(right) == Some(0) {
        left.as_ref()
    } else if constant_value(left) == Some(0) {
        right.as_ref()
    } else {
        return None;
    };
    let (tested_previous_offset, tested_previous_type) =
        direct_member(tested_previous, &cell.name)?;
    let [Statement::Return(Some(empty_result))] = previous_body.as_slice() else {
        return None;
    };
    let (empty_next_offset, empty_next_type) = direct_member(empty_result, &cell.name)?;
    if tested_previous_offset != previous_offset
        || empty_next_offset != next_offset
        || !pointer_word(tested_previous_type)
        || !pointer_word(empty_next_type)
    {
        return None;
    }

    let Statement::Store {
        target: previous_next,
        value: current_next,
    } = final_store
    else {
        return None;
    };
    let (previous_base_offset, previous_base_type, repaired_next_offset, repaired_next_type) =
        nested_member(previous_next, &cell.name)?;
    let (current_next_offset, current_next_type) = direct_member(current_next, &cell.name)?;
    if previous_base_offset != previous_offset
        || repaired_next_offset != next_offset
        || current_next_offset != next_offset
        || ![previous_base_type, repaired_next_type, current_next_type]
            .into_iter()
            .all(pointer_word)
    {
        return None;
    }

    Some(DoublyLinkedListExtract {
        list: &list.name,
        cell: &cell.name,
        previous_offset,
        next_offset,
    })
}

impl Generator {
    pub(crate) fn try_doubly_linked_list_extract(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = summarize(function) else {
            return Ok(false);
        };
        let list = self
            .lookup_general(plan.list)
            .ok_or_else(|| Diagnostic::error("linked-list head is not in a general register"))?;
        let cell = self
            .lookup_general(plan.cell)
            .ok_or_else(|| Diagnostic::error("linked-list cell is not in a general register"))?;
        if list != Eabi::general_result().number || cell != 4 {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let after_next_repair = self.fresh_label();
        let repair_previous = self.fresh_label();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: cell,
                offset: plan.next_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: cell,
                offset: plan.previous_offset,
            },
            Instruction::StoreWord {
                s: GENERAL_SCRATCH,
                a: 5,
                offset: plan.previous_offset,
            },
        ]);
        self.bind_label(after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: cell,
                offset: plan.previous_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, repair_previous);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: Eabi::general_result().number,
                a: cell,
                offset: plan.next_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        self.bind_label(repair_previous);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: cell,
                offset: plan.next_offset,
            },
            Instruction::StoreWord {
                s: GENERAL_SCRATCH,
                a: 5,
                offset: plan.next_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
