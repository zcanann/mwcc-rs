//! Sign-directed floating member decay with a zero-crossing clamp.
//!
//! MWCC keeps the first member load and the zero literal live through both
//! arms. Each arm stores and reloads the updated member, then returns directly
//! when it did not cross zero. Owning the complete diamond avoids teaching the
//! ordinary statement walker partial value lifetimes across nested branches.

#[allow(unused_imports)]
use super::*;

struct FloatMember<'a> {
    base: &'a str,
    offset: i16,
}

fn float_member(expression: &Expression) -> Option<FloatMember<'_>> {
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
    Some(FloatMember {
        base,
        offset: i16::try_from(*offset).ok()?,
    })
}

fn same_member(expression: &Expression, expected: &FloatMember<'_>) -> bool {
    float_member(expression)
        .is_some_and(|member| member.base == expected.base && member.offset == expected.offset)
}

fn update_store(
    statement: &Statement,
    member: &FloatMember<'_>,
    adjustment: &str,
    operator: BinaryOperator,
) -> bool {
    matches!(statement,
        Statement::Store {
            target,
            value: Expression::Binary { operator: actual, left, right },
        } if *actual == operator
            && same_member(target, member)
            && same_member(left, member)
            && matches!(right.as_ref(), Expression::Variable(name) if name == adjustment))
}

fn zero_crossing_clamp(
    statement: &Statement,
    member: &FloatMember<'_>,
    operator: BinaryOperator,
) -> bool {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: actual,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    matches!(then_body.as_slice(), [Statement::Store { target, value }]
        if same_member(target, member) && is_zero_literal(value))
        && else_body.is_empty()
        && *actual == operator
        && same_member(left, member)
        && is_zero_literal(right)
}

fn classify(function: &Function) -> Option<FloatMember<'_>> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, adjustment] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || adjustment.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let member = float_member(left)?;
    if member.base != base.name
        || !is_zero_literal(right)
        || !matches!(then_body.as_slice(), [update, clamp]
            if update_store(update, &member, &adjustment.name, BinaryOperator::Add)
                && zero_crossing_clamp(clamp, &member, BinaryOperator::Greater))
        || !matches!(else_body.as_slice(), [update, clamp]
            if update_store(update, &member, &adjustment.name, BinaryOperator::Subtract)
                && zero_crossing_clamp(clamp, &member, BinaryOperator::Less))
    {
        return None;
    }
    Some(member)
}

impl Generator {
    pub(crate) fn try_symmetric_float_decay(&mut self, function: &Function) -> Compilation<bool> {
        let Some(member) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(member.base)?;
        let adjustment = self.float_register_of(&function.parameters[1].name)?;
        if base != 3 || adjustment != 1 {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: base,
            offset: member.offset,
        });
        self.load_float_constant(2, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 2 });
        let nonnegative = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, nonnegative); // bge

        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: 0,
            b: adjustment,
        });
        self.emit_decay_store_and_reload(base, member.offset);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            }); // blelr
        self.emit_decay_zero_and_return(base, member.offset);

        self.bind_label(nonnegative);
        self.output.instructions.push(Instruction::FloatSubtractSingle {
            d: 0,
            a: 0,
            b: adjustment,
        });
        self.emit_decay_store_and_reload(base, member.offset);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            }); // bgelr
        self.emit_decay_zero_and_return(base, member.offset);
        Ok(true)
    }

    fn emit_decay_store_and_reload(&mut self, base: u8, offset: i16) {
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: 0,
            a: base,
            offset,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: base,
            offset,
        });
    }

    fn emit_decay_zero_and_return(&mut self, base: u8, offset: i16) {
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: 2,
            a: base,
            offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn recognizes_sign_directed_decay_with_zero_crossing_clamps() {
        let member = |offset| Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        };
        let update = |operator| Statement::Store {
            target: member(32),
            value: Expression::Binary {
                operator,
                left: Box::new(member(32)),
                right: Box::new(Expression::Variable("amount".into())),
            },
        };
        let clamp = |operator| Statement::If {
            condition: Expression::Binary {
                operator,
                left: Box::new(member(32)),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Store {
                target: member(32),
                value: Expression::IntegerLiteral(0),
            }],
            else_body: vec![],
        };
        let function = Function {
            return_type: Type::Void,
            name: "decay".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 64 },
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "amount".into(),
                },
            ],
            locals: vec![],
            statements: vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(member(32)),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![update(BinaryOperator::Add), clamp(BinaryOperator::Greater)],
                else_body: vec![update(BinaryOperator::Subtract), clamp(BinaryOperator::Less)],
            }],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(classify(&function).is_some());
    }
}
