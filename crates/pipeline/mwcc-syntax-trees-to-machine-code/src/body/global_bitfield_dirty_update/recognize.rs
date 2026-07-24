//! Structural recognition for global bitfield and dirty-mask pairs.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct GlobalBitfieldDirty<'a> {
    pub(super) parameter: &'a str,
    pub(super) global: &'a str,
    pub(super) field_offset: i16,
    pub(super) dirty_offset: i16,
    pub(super) dirty_mask: u16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
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
    let [noop, field, dirty] = function.statements.as_slice() else {
        return None;
    };
    if !no_op(noop) {
        return None;
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: field_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    } = single_iteration(field)
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
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
        || !matches!(stripped(old), Expression::Member {
            base,
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } if offset == field_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == global))
        || !matches!(stripped(inserted), Expression::Variable(name) if name == &parameter.name)
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
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: dirty_old,
                right: dirty_mask,
            },
    } = dirty
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
    })
}
