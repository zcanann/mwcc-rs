//! Pointer-walk reductions over a nested guarded value.
//!
//! SDK priority inheritance computes a running minimum while walking an
//! intrusive mutex list. The loop is a useful general leaf topology: one
//! pointer cursor, one guarded nested pointer, and one scalar reduction. Keep
//! its semantic recognition separate from emission so unrelated reassignment
//! loops continue to decline without partially mutating the output.

#[allow(unused_imports)]
use super::*;

struct PointerWalkMinimum {
    initial_offset: i16,
    cursor_head_offset: i16,
    nested_head_offset: i16,
    candidate_value_offset: i16,
    cursor_next_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn assignment(expression: &Expression) -> Option<(&str, &Expression)> {
    match expression {
        Expression::Assign { target, value } => Some((variable(target)?, value)),
        _ => None,
    }
}

fn direct_member<'a>(expression: &'a Expression, base_name: &str) -> Option<(i16, &'a Type)> {
    match expression {
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } if variable(base) == Some(base_name) => {
            Some((i16::try_from(*offset).ok()?, member_type))
        }
        _ => None,
    }
}

fn is_general_pointer(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

fn classify(function: &Function) -> Option<PointerWalkMinimum> {
    if !function.guards.is_empty()
        || function_makes_call(function)
        || function.parameters.len() != 1
        || function.locals.len() != 3
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let parameter = &function.parameters[0];
    if !is_general_pointer(parameter.parameter_type) {
        return None;
    }
    let [Statement::Assign {
        name: minimum,
        value: initial,
    }, Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(loop_initializer),
        condition: Some(loop_condition),
        step: Some(loop_step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if variable(function.return_expression.as_ref()?) != Some(minimum) {
        return None;
    }
    let minimum_local = function.locals.iter().find(|local| local.name == *minimum)?;
    if !matches!(
        minimum_local.declared_type,
        Type::Char
            | Type::UnsignedChar
            | Type::Short
            | Type::UnsignedShort
            | Type::Int
            | Type::UnsignedInt
    ) || function.return_type != minimum_local.declared_type
    {
        return None;
    }
    let (initial_offset, initial_type) = direct_member(initial, &parameter.name)?;
    if *initial_type != minimum_local.declared_type {
        return None;
    }

    let (cursor, cursor_head) = assignment(loop_initializer)?;
    let cursor_local = function.locals.iter().find(|local| local.name == cursor)?;
    if !is_general_pointer(cursor_local.declared_type)
        || variable(loop_condition) != Some(cursor)
    {
        return None;
    }
    let (cursor_head_offset, cursor_head_type) =
        direct_member(cursor_head, &parameter.name)?;
    if !is_general_pointer(*cursor_head_type) {
        return None;
    }

    let (step_cursor, cursor_next) = assignment(loop_step)?;
    let (cursor_next_offset, cursor_next_type) = direct_member(cursor_next, cursor)?;
    if step_cursor != cursor || !is_general_pointer(*cursor_next_type) {
        return None;
    }

    let [Statement::Assign {
        name: candidate,
        value: nested_head,
    }, Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: candidate_guard,
                right: comparison,
            },
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    let candidate_local = function
        .locals
        .iter()
        .find(|local| local.name == *candidate)?;
    if !is_general_pointer(candidate_local.declared_type)
        || variable(candidate_guard) != Some(candidate)
        || !else_body.is_empty()
    {
        return None;
    }
    let (nested_head_offset, nested_head_type) = direct_member(nested_head, cursor)?;
    if !is_general_pointer(*nested_head_type) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: candidate_value,
        right: current_minimum,
    } = comparison.as_ref()
    else {
        return None;
    };
    let (candidate_value_offset, candidate_value_type) =
        direct_member(candidate_value, candidate)?;
    if *candidate_value_type != minimum_local.declared_type
        || variable(current_minimum) != Some(minimum)
    {
        return None;
    }
    let [Statement::Assign {
        name: updated_minimum,
        value: updated_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let (updated_offset, updated_type) = direct_member(updated_value, candidate)?;
    if updated_minimum != minimum
        || updated_offset != candidate_value_offset
        || updated_type != candidate_value_type
    {
        return None;
    }

    Some(PointerWalkMinimum {
        initial_offset,
        cursor_head_offset,
        nested_head_offset,
        candidate_value_offset,
        cursor_next_offset,
    })
}

impl Generator {
    pub(crate) fn try_pointer_walk_running_minimum(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(walk) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let root = self.general_register_of(&function.parameters[0].name)?;
        if root != Eabi::FIRST_GENERAL_ARGUMENT {
            return Ok(false);
        }

        const MINIMUM: u8 = 4;
        let (cursor, candidate) = if self.behavior.legacy_pointer_value_register_order {
            (5, 3)
        } else {
            (3, 5)
        };
        self.output.pre_scheduled = true;
        // A rotated `for` contributes four internal labels; the guarded
        // short-circuit update contributes three more.
        self.output.anonymous_label_bump = 7;

        self.output.instructions.extend([
            Instruction::LoadWord {
                d: MINIMUM,
                a: root,
                offset: walk.initial_offset,
            },
            Instruction::LoadWord {
                d: cursor,
                a: root,
                offset: walk.cursor_head_offset,
            },
            Instruction::Branch { target: 11 },
            Instruction::LoadWord {
                d: candidate,
                a: cursor,
                offset: walk.nested_head_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: candidate,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 10,
            },
            Instruction::LoadWord {
                d: 0,
                a: candidate,
                offset: walk.candidate_value_offset,
            },
            Instruction::CompareWord {
                a: 0,
                b: MINIMUM,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 10,
            },
            Instruction::Or {
                a: MINIMUM,
                s: 0,
                b: 0,
            },
            Instruction::LoadWord {
                d: cursor,
                a: cursor,
                offset: walk.cursor_next_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: cursor,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 3,
            },
            Instruction::Or {
                a: Eabi::general_result().number,
                s: MINIMUM,
                b: MINIMUM,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
