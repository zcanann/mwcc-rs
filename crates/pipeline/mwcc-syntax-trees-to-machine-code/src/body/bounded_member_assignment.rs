//! Leaf assignment guarded by a signed lower bound and member upper bound.
//!
//! The source `if (value >= LOW && value < object->limit)` remains two
//! short-circuit branches. On success MWCC stores the already-resident value and
//! returns one constant; both failures share the trailing constant-return block.

#[allow(unused_imports)]
use super::*;

struct BoundedMemberAssignment<'a> {
    base: &'a str,
    value: &'a str,
    lower: i16,
    limit_offset: i16,
    target_offset: i16,
    success: i16,
    failure: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&str, i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((variable(base)?, i16::try_from(*offset).ok()?, *member_type))
}

fn classify(function: &Function) -> Option<BoundedMemberAssignment<'_>> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: lower_value,
        right: lower,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: upper_value,
        right: limit,
    } = right.as_ref()
    else {
        return None;
    };
    let value = variable(lower_value)?;
    if variable(upper_value)? != value {
        return None;
    }
    let lower = i16::try_from(constant_value(lower)?).ok()?;
    let (base, limit_offset, limit_type) = member(limit)?;
    if !matches!(limit_type, Type::Int | Type::UnsignedInt) {
        return None;
    }
    let [Statement::Store {
        target,
        value: stored,
    }, Statement::Return(Some(success))] = then_body.as_slice()
    else {
        return None;
    };
    let (target_base, target_offset, target_type) = member(target)?;
    if target_base != base
        || variable(stored)? != value
        || !matches!(target_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }
    Some(BoundedMemberAssignment {
        base,
        value,
        lower,
        limit_offset,
        target_offset,
        success: i16::try_from(constant_value(success)?).ok()?,
        failure: i16::try_from(constant_value(function.return_expression.as_ref()?)?).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_bounded_member_assignment(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let Some(base) = self.lookup_general(shape.base) else {
            return Ok(false);
        };
        let Some(value) = self.lookup_general(shape.value) else {
            return Ok(false);
        };
        if base == GENERAL_SCRATCH || value == GENERAL_SCRATCH {
            return Ok(false);
        }

        let failure = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: value,
                immediate: shape.lower,
            });
        self.emit_branch_conditional_to(12, 0, failure);
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: base,
            offset: shape.limit_offset,
        });
        self.output.instructions.push(Instruction::CompareWord {
            a: value,
            b: GENERAL_SCRATCH,
        });
        self.emit_branch_conditional_to(4, 0, failure);
        self.output.instructions.push(Instruction::StoreWord {
            s: value,
            a: base,
            offset: shape.target_offset,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shape.success));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shape.failure));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
