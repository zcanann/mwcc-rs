//! Structural recognition for global bitfield and dirty-mask pairs.

#[allow(unused_imports)]
use super::super::*;

#[derive(Clone, Copy)]
pub(super) enum FieldUpdate {
    ShiftOr,
    RotateInsert { shift: u8, begin: u8, end: u8 },
}

pub(super) struct GlobalBitfieldDirty<'a> {
    pub(super) parameter: &'a str,
    pub(super) global: &'a str,
    pub(super) field_offset: i16,
    pub(super) dirty_offset: i16,
    pub(super) dirty_mask: u16,
    pub(super) update: FieldUpdate,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn word_value(mut expression: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = expression
    {
        expression = operand;
    }
    expression
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if constant_value(operand) == Some(0))
}

fn single_iteration(statement: &Statement) -> &Statement {
    if let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    {
        if constant_value(condition) == Some(0) {
            if let [statement] = body.as_slice() {
                return statement;
            }
        }
    }
    statement
}

pub(super) fn recognize(function: &Function) -> Option<GlobalBitfieldDirty<'_>> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    if parameter.parameter_type != Type::UnsignedChar {
        return None;
    }
    let (field, dirty, has_noop) = match function.statements.as_slice() {
        [noop, field, dirty] if no_op(noop) => (field, dirty, true),
        [field, dirty] => (field, dirty, false),
        _ => return None,
    };
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: field_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value,
    } = single_iteration(field)
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    let (old, inserted, update) = if let Expression::Call { name, arguments } = word_value(value) {
        if crate::intrinsics::classify(name, arguments.len())
            != Some(crate::intrinsics::Intrinsic::RotateLeftWordInsert)
        {
            return None;
        }
        let insert = crate::intrinsics::rotate_insert(arguments)?;
        (
            word_value(insert.initial),
            word_value(insert.source),
            FieldUpdate::RotateInsert {
                shift: insert.shift,
                begin: insert.begin,
                end: insert.end,
            },
        )
    } else {
        // The measured C form ORs the entire shifted byte, including bits
        // outside the cleared three-bit field. It is not an rlwimi equivalent.
        if !has_noop {
            return None;
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left,
            right,
        } = value
        else {
            return None;
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: old,
            right: preserve,
        } = left.as_ref()
        else {
            return None;
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: inserted,
            right: shift,
        } = right.as_ref()
        else {
            return None;
        };
        if constant_value(preserve).map(|value| value as u32) != Some(0xfff8_ffff)
            || constant_value(shift) != Some(16)
        {
            return None;
        }
        (stripped(old), stripped(inserted), FieldUpdate::ShiftOr)
    };
    if !matches!(old, Expression::Member {
            base, offset, member_type: Type::UnsignedInt, index_stride: None,
        } if offset == field_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == global))
        || !matches!(inserted, Expression::Variable(name) if name == &parameter.name)
    {
        return None;
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: dirty_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value: dirty_value,
    } = dirty
    else {
        return None;
    };
    let dirty_value = match dirty_value {
        Expression::IndexedUpdateValue { value } => value.as_ref(),
        value => value,
    };
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: dirty_old,
        right: dirty_mask,
    } = dirty_value
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == global)
        || !matches!(dirty_old.as_ref(), Expression::Member {
            base,
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } if offset == dirty_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == global))
    {
        return None;
    }
    Some(GlobalBitfieldDirty {
        parameter: &parameter.name,
        global,
        field_offset: i16::try_from(*field_offset).ok()?,
        dirty_offset: i16::try_from(*dirty_offset).ok()?,
        dirty_mask: u16::try_from(constant_value(dirty_mask)?).ok()?,
        update,
    })
}
