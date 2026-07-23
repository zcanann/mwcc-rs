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

enum Inputs<'a> {
    Parameters {
        adjustment: &'a str,
        bound: &'a str,
    },
    Members {
        adjustment: Member<'a>,
        bound: Member<'a>,
    },
}

struct LeadingFloatUpdateClamp<'a> {
    member: Member<'a>,
    inputs: Inputs<'a>,
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

fn update_clamp_member<'a>(
    update_statement: &Statement,
    clamp_statement: &Statement,
    base: &'a str,
    adjustment: &str,
    bound: &str,
) -> Option<(Member<'a>, BinaryOperator, BinaryOperator, bool)> {
    let Statement::Store {
        target,
        value:
            Expression::Binary {
                operator: update,
                left: update_member,
                right: update_amount,
            },
    } = update_statement
    else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: comparison,
                left: compared_member,
                right: compared_bound,
            },
        then_body,
        else_body,
    } = clamp_statement
    else {
        return None;
    };
    let member = float_member(target)?;
    if member.base != base
        || !same_member(update_member, &member)
        || !matches!(update_amount.as_ref(), Expression::Variable(name) if name == adjustment)
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
    let negate_bound = bound_expression(compared_bound, bound)?;
    let [Statement::Store {
        target: clamped_member,
        value: clamped_bound,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !same_member(clamped_member, &member)
        || bound_expression(clamped_bound, bound)? != negate_bound
    {
        return None;
    }
    Some((
        Member {
            base,
            offset: member.offset,
        },
        *update,
        *comparison,
        negate_bound,
    ))
}

fn automatic_float(local: &mwcc_syntax_trees::LocalDeclaration) -> bool {
    local.declared_type == Type::Float
        && !local.is_volatile
        && local.array_length.is_none()
        && !local.is_static
}

fn classify(function: &Function) -> Option<LeadingFloatUpdateClamp<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    match function.parameters.as_slice() {
        [base, adjustment, bound]
            if matches!(
                base.parameter_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            ) && adjustment.parameter_type == Type::Float
                && bound.parameter_type == Type::Float
                && function.locals.is_empty() =>
        {
            let [update_statement, clamp_statement] = function.statements.as_slice() else {
                return None;
            };
            let (member, update, comparison, negate_bound) = update_clamp_member(
                update_statement,
                clamp_statement,
                &base.name,
                &adjustment.name,
                &bound.name,
            )?;
            Some(LeadingFloatUpdateClamp {
                member,
                inputs: Inputs::Parameters {
                    adjustment: &adjustment.name,
                    bound: &bound.name,
                },
                update,
                comparison,
                negate_bound,
            })
        }
        [base]
            if matches!(
                base.parameter_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            ) =>
        {
            let [adjustment_local, bound_local] = function.locals.as_slice() else {
                return None;
            };
            if !automatic_float(adjustment_local)
                || !automatic_float(bound_local)
                || adjustment_local.initializer.is_some()
                || bound_local.initializer.is_some()
            {
                return None;
            }
            let [Statement::Assign {
                name: assigned_adjustment,
                value: adjustment_value,
            }, Statement::Assign {
                name: assigned_bound,
                value: bound_value,
            }, update, clamp] = function.statements.as_slice()
            else {
                return None;
            };
            if assigned_adjustment != &adjustment_local.name
                || assigned_bound != &bound_local.name
            {
                return None;
            }
            let adjustment_member = float_member(adjustment_value)?;
            let bound_member = float_member(bound_value)?;
            if adjustment_member.base != base.name || bound_member.base != base.name {
                return None;
            }
            let (member, update_operator, comparison, negate_bound) = update_clamp_member(
                update,
                clamp,
                &base.name,
                &adjustment_local.name,
                &bound_local.name,
            )?;
            if update_operator != BinaryOperator::Subtract
                || comparison != BinaryOperator::Less
                || !negate_bound
            {
                return None;
            }
            Some(LeadingFloatUpdateClamp {
                member,
                inputs: Inputs::Members {
                    adjustment: adjustment_member,
                    bound: bound_member,
                },
                update: update_operator,
                comparison,
                negate_bound,
            })
        }
        _ => None,
    }
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
        let bound = match shape.inputs {
            Inputs::Parameters { adjustment, bound } => {
                let adjustment = self.float_register_of(adjustment)?;
                let bound = self.float_register_of(bound)?;
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
                bound
            }
            Inputs::Members { adjustment, bound } => {
                if base != 3 || adjustment.base != shape.member.base || bound.base != shape.member.base
                {
                    return Ok(false);
                }
                self.output.pre_scheduled = true;
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 1,
                    a: base,
                    offset: shape.member.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: base,
                    offset: adjustment.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 2,
                    a: base,
                    offset: bound.offset,
                });
                self.output.instructions.push(Instruction::FloatSubtractSingle {
                    d: 0,
                    a: 1,
                    b: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::FloatNegate { d: 1, b: 2 });
                1
            }
        };
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
