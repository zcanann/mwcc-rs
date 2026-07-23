//! Symmetric lower/upper clamping of one floating-point member.
//!
//! MWCC keeps the negated bound and member value live across both comparisons,
//! returns directly after the lower store, and reuses the first member load for
//! the upper test. A source-level local changes which scratch register receives
//! each value even though it does not otherwise survive lowering.

#[allow(unused_imports)]
use super::*;

struct Member<'a> {
    base: &'a str,
    offset: i16,
}

enum Bound<'a> {
    Parameter {
        name: &'a str,
        source_local: bool,
    },
    Member(Member<'a>),
}

struct SymmetricFloatClamp<'a> {
    member: Member<'a>,
    bound: Bound<'a>,
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

fn negated_variable(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } if matches!(operand.as_ref(), Expression::Variable(name) if name == expected)
    )
}

fn clamp_member<'a>(
    statements: &[Statement],
    base: &'a str,
    bound: &str,
    value: impl Fn(&Expression, &Member<'_>) -> bool,
) -> Option<Member<'a>> {
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: lower_value,
                right: lower_bound,
            },
        then_body: lower_body,
        else_body,
    }] = statements
    else {
        return None;
    };
    let [Statement::Store {
        target: lower_target,
        value: lower_store,
    }] = lower_body.as_slice()
    else {
        return None;
    };
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: upper_value,
                right: upper_bound,
            },
        then_body: upper_body,
        else_body: upper_else,
    }] = else_body.as_slice()
    else {
        return None;
    };
    let [Statement::Store {
        target: upper_target,
        value: Expression::Variable(upper_store),
    }] = upper_body.as_slice()
    else {
        return None;
    };
    if !upper_else.is_empty()
        || !negated_variable(lower_bound, bound)
        || !negated_variable(lower_store, bound)
        || !matches!(upper_bound.as_ref(), Expression::Variable(name) if name == bound)
        || upper_store != bound
    {
        return None;
    }
    let member = float_member(lower_target)?;
    if member.base != base
        || !same_member(upper_target, &member)
        || !value(lower_value, &member)
        || !value(upper_value, &member)
    {
        return None;
    }
    Some(Member {
        base,
        offset: member.offset,
    })
}

fn automatic_float(local: &mwcc_syntax_trees::LocalDeclaration) -> bool {
    local.declared_type == Type::Float
        && !local.is_volatile
        && local.array_length.is_none()
        && !local.is_static
}

fn classify(function: &Function) -> Option<SymmetricFloatClamp<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    match function.parameters.as_slice() {
        [base, bound]
            if matches!(base.parameter_type, Type::StructPointer { .. })
                && bound.parameter_type == Type::Float =>
        {
            let (member, source_local) = match function.locals.as_slice() {
                [] => {
                    let member = clamp_member(
                        &function.statements,
                        &base.name,
                        &bound.name,
                        |expression, member| same_member(expression, member),
                    )?;
                    (member, false)
                }
                [local]
                    if automatic_float(local)
                        && local.initializer.as_ref().is_some_and(|initializer| {
                            float_member(initializer)
                                .is_some_and(|candidate| candidate.base == base.name)
                        }) =>
                {
                    let member = float_member(local.initializer.as_ref()?)?;
                    let member = clamp_member(
                        &function.statements,
                        &base.name,
                        &bound.name,
                        |expression, _| {
                            matches!(expression, Expression::Variable(name) if name == &local.name)
                        },
                    )
                    .filter(|candidate| {
                        candidate.base == member.base && candidate.offset == member.offset
                    })?;
                    (member, true)
                }
                _ => return None,
            };
            Some(SymmetricFloatClamp {
                member,
                bound: Bound::Parameter {
                    name: &bound.name,
                    source_local,
                },
            })
        }
        [base] if matches!(base.parameter_type, Type::StructPointer { .. }) => {
            let [bound_local, value_local] = function.locals.as_slice() else {
                return None;
            };
            if !automatic_float(bound_local)
                || !automatic_float(value_local)
                || bound_local.initializer.is_some()
                || value_local.initializer.is_some()
            {
                return None;
            }
            let [Statement::Assign {
                name: assigned_bound,
                value: bound_value,
            }, Statement::Assign {
                name: assigned_value,
                value: member_value,
            }, clamp] = function.statements.as_slice()
            else {
                return None;
            };
            if assigned_bound != &bound_local.name || assigned_value != &value_local.name {
                return None;
            }
            let bound_member = float_member(bound_value)?;
            let loaded_member = float_member(member_value)?;
            if bound_member.base != base.name || loaded_member.base != base.name {
                return None;
            }
            let member = clamp_member(
                std::slice::from_ref(clamp),
                &base.name,
                &bound_local.name,
                |expression, _| {
                    matches!(expression, Expression::Variable(name) if name == &value_local.name)
                },
            )
            .filter(|candidate| {
                candidate.base == loaded_member.base && candidate.offset == loaded_member.offset
            })?;
            Some(SymmetricFloatClamp {
                member,
                bound: Bound::Member(bound_member),
            })
        }
        _ => None,
    }
}

impl Generator {
    pub(crate) fn try_symmetric_float_clamp(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.member.base)?;
        if base != 3 {
            return Ok(false);
        }
        let (bound, negative, member) = match shape.bound {
            Bound::Parameter { name, source_local } => {
                let bound = self.float_register_of(name)?;
                if bound != 1 {
                    return Ok(false);
                }
                self.output.pre_scheduled = true;
                let (negative, member) = if source_local { (2, 0) } else { (0, 2) };
                self.output
                    .instructions
                    .push(Instruction::FloatNegate { d: negative, b: bound });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: member,
                    a: base,
                    offset: shape.member.offset,
                });
                (bound, negative, member)
            }
            Bound::Member(bound_member) => {
                if bound_member.base != shape.member.base {
                    return Ok(false);
                }
                self.output.pre_scheduled = true;
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 1,
                    a: base,
                    offset: bound_member.offset,
                });
                self.output.instructions.push(Instruction::LoadFloatSingle {
                    d: 2,
                    a: base,
                    offset: shape.member.offset,
                });
                self.output
                    .instructions
                    .push(Instruction::FloatNegate { d: 0, b: 1 });
                (1, 0, 2)
            }
        };
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: member,
                b: negative,
            });
        let (options, condition_bit) = false_branch_bo_bi(BinaryOperator::Less)
            .expect("an ordered less-than comparison has a false branch");
        let lower_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: negative,
            a: base,
            offset: shape.member.offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let upper_test = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[lower_branch]
        {
            *target = upper_test;
        }
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: member,
                b: bound,
            });
        let (options, condition_bit) = false_branch_bo_bi(BinaryOperator::Greater)
            .expect("an ordered greater-than comparison has a false branch");
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
