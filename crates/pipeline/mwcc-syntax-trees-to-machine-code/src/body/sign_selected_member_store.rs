//! Store a floating-point sign selected from another member.
//!
//! A one-use local assigned `+1`/`-1` by a member-vs-zero diamond lives in f0
//! in MWCC. The compared member occupies f1 only through the condition; treating
//! the source local as an ordinary allocated value incorrectly keeps it in f1.

#[allow(unused_imports)]
use super::*;

struct Member<'a> {
    base: &'a str,
    offset: i16,
}

struct SignSelectedMemberStore<'a> {
    condition: Member<'a>,
    target: Member<'a>,
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

fn integer_value(expression: &Expression, expected: i64) -> bool {
    matches!(expression, Expression::IntegerLiteral(value) if *value == expected)
}

fn classify(function: &Function) -> Option<SignSelectedMemberStore<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::StructPointer { .. }) {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if local.declared_type != Type::Float
        || local.initializer.is_some()
        || local.is_volatile
        || local.array_length.is_some()
        || local.is_static
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: condition_member,
                right: condition_zero,
            },
        then_body,
        else_body,
    }, Statement::Store {
        target,
        value: Expression::Variable(stored_local),
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [Statement::Assign {
        name: then_local,
        value: then_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [Statement::Assign {
        name: else_local,
        value: else_value,
    }] = else_body.as_slice()
    else {
        return None;
    };
    if !integer_value(condition_zero, 0)
        || then_local != &local.name
        || else_local != &local.name
        || stored_local != &local.name
        || !integer_value(then_value, 1)
        || !integer_value(else_value, -1)
    {
        return None;
    }
    let condition = float_member(condition_member)?;
    let target = float_member(target)?;
    if condition.base != base.name || target.base != base.name {
        return None;
    }
    Some(SignSelectedMemberStore { condition, target })
}

impl Generator {
    pub(crate) fn try_sign_selected_member_store(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.condition.base)?;
        if base != 3 || shape.target.base != shape.condition.base {
            return Ok(false);
        }
        self.output.pre_scheduled = true;

        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: base,
            offset: shape.condition.offset,
        });
        self.load_float_literal(FLOAT_SCRATCH, 0.0, false);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: 1,
                b: FLOAT_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        let else_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.load_float_literal(FLOAT_SCRATCH, 1.0, false);
        let join_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[else_branch]
        {
            *target = else_label;
        }
        self.load_float_literal(FLOAT_SCRATCH, -1.0, false);
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
            *target = join;
        }
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: FLOAT_SCRATCH,
            a: base,
            offset: shape.target.offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
