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

struct SymmetricFloatClamp<'a> {
    member: Member<'a>,
    bound: &'a str,
    source_local: bool,
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

fn classify(function: &Function) -> Option<SymmetricFloatClamp<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, bound] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::StructPointer { .. })
        || bound.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: lower_value,
                right: lower_bound,
            },
        then_body: lower_body,
        else_body,
    }] = function.statements.as_slice()
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
        || !negated_variable(lower_bound, &bound.name)
        || !negated_variable(lower_store, &bound.name)
        || !matches!(upper_bound.as_ref(), Expression::Variable(name) if name == &bound.name)
        || upper_store != &bound.name
    {
        return None;
    }
    let member = float_member(lower_target)?;
    if member.base != base.name
        || !same_member(upper_target, &member)
    {
        return None;
    }
    let source_local = match function.locals.as_slice() {
        [] if same_member(lower_value, &member) && same_member(upper_value, &member) => false,
        [local]
            if local.declared_type == Type::Float
                && !local.is_volatile
                && local.array_length.is_none()
                && !local.is_static
                && local
                    .initializer
                    .as_ref()
                    .is_some_and(|initializer| same_member(initializer, &member)) =>
        {
            if !matches!(lower_value.as_ref(), Expression::Variable(name) if name == &local.name)
                || !matches!(upper_value.as_ref(), Expression::Variable(name) if name == &local.name)
            {
                return None;
            }
            true
        }
        _ => return None,
    };
    Some(SymmetricFloatClamp {
        member,
        bound: &bound.name,
        source_local,
    })
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
        let bound = self.float_register_of(shape.bound)?;
        if base != 3 || bound != 1 {
            return Ok(false);
        }
        let (negative, member) = if shape.source_local { (2, 0) } else { (0, 2) };
        self.output.pre_scheduled = true;

        self.output
            .instructions
            .push(Instruction::FloatNegate { d: negative, b: bound });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: member,
            a: base,
            offset: shape.member.offset,
        });
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
