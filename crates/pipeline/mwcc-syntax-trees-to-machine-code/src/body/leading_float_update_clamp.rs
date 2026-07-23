//! In-place float updates followed by a clamp on the updated member.
//!
//! MWCC treats the update and trailing clamp as one scheduling region. In the
//! lower-bound form, it negates the bound while the member load is in flight;
//! both forms then store and reload the member before the comparison.

#[allow(unused_imports)]
use super::*;

struct Member<'a> {
    base: &'a str,
    offset: i16,
}

struct LeadingFloatUpdateClamp<'a> {
    member: Member<'a>,
    adjustment: &'a str,
    bound: &'a str,
    update: BinaryOperator,
    comparison: BinaryOperator,
    negate_bound: bool,
}

fn float_member(expression: &Expression) -> Option<Member<'_>> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some(Member {
        base,
        offset: i16::try_from(*offset).ok()?,
    })
}

fn same_member(expression: &Expression, expected: &Member<'_>) -> bool {
    float_member(expression)
        .is_some_and(|member| member.base == expected.base && member.offset == expected.offset)
}

fn bound_expression(expression: &Expression, expected: &str) -> Option<bool> {
    match expression {
        Expression::Variable(name) if name == expected => Some(false),
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } if matches!(operand.as_ref(), Expression::Variable(name) if name == expected) => {
            Some(true)
        }
        _ => None,
    }
}

fn classify(function: &Function) -> Option<LeadingFloatUpdateClamp<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, adjustment, bound] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        base.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || adjustment.parameter_type != Type::Float
        || bound.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::Store {
        target,
        value:
            Expression::Binary {
                operator: update,
                left: update_member,
                right: update_amount,
            },
    }, Statement::If {
        condition:
            Expression::Binary {
                operator: comparison,
                left: compared_member,
                right: compared_bound,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let member = float_member(target)?;
    if member.base != base.name
        || !same_member(update_member, &member)
        || !matches!(update_amount.as_ref(), Expression::Variable(name) if name == &adjustment.name)
        || !same_member(compared_member, &member)
        || !else_body.is_empty()
        || !matches!(
            (*update, *comparison),
            (BinaryOperator::Add, BinaryOperator::Greater)
                | (BinaryOperator::Subtract, BinaryOperator::Less)
        )
    {
        return None;
    }
    let negate_bound = bound_expression(compared_bound, &bound.name)?;
    let [Statement::Store {
        target: clamped_member,
        value: clamped_bound,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !same_member(clamped_member, &member)
        || bound_expression(clamped_bound, &bound.name)? != negate_bound
    {
        return None;
    }
    Some(LeadingFloatUpdateClamp {
        member,
        adjustment: &adjustment.name,
        bound: &bound.name,
        update: *update,
        comparison: *comparison,
        negate_bound,
    })
}

impl Generator {
    pub(crate) fn try_leading_float_update_clamp(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.member.base)?;
        let adjustment = self.float_register_of(shape.adjustment)?;
        let bound = self.float_register_of(shape.bound)?;
        self.output.pre_scheduled = true;

        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: FLOAT_SCRATCH,
            a: base,
            offset: shape.member.offset,
        });
        if shape.negate_bound {
            self.output
                .instructions
                .push(Instruction::FloatNegate { d: bound, b: bound });
        }
        self.output.instructions.push(match shape.update {
            BinaryOperator::Add => Instruction::FloatAddSingle {
                d: FLOAT_SCRATCH,
                a: FLOAT_SCRATCH,
                b: adjustment,
            },
            BinaryOperator::Subtract => Instruction::FloatSubtractSingle {
                d: FLOAT_SCRATCH,
                a: FLOAT_SCRATCH,
                b: adjustment,
            },
            _ => unreachable!("the classifier accepts only add and subtract"),
        });
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: FLOAT_SCRATCH,
            a: base,
            offset: shape.member.offset,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: FLOAT_SCRATCH,
            a: base,
            offset: shape.member.offset,
        });
        self.output.instructions.push(Instruction::FloatCompareOrdered {
            a: FLOAT_SCRATCH,
            b: bound,
        });
        let (options, condition_bit) = false_branch_bo_bi(shape.comparison)
            .expect("the classifier accepts only ordered comparisons");
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options,
                condition_bit,
            });
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: bound,
            a: base,
            offset: shape.member.offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
