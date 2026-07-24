//! A nullable object guard whose first return condition compares a float member
//! with a floating call result, followed by a one-bit member test.

#[allow(unused_imports)]
use super::*;

struct Plan<'a> {
    member_offset: i16,
    call: &'a str,
    argument: f32,
    first_result: i16,
    word_offset: i16,
    shift: u8,
    bit_result: i16,
    clear_result: i16,
    null_result: i16,
}

impl Generator {
    /// Lower the optimized LinkageFirst schedule as one control-flow owner.
    ///
    /// The object address must survive the floating call, every return shares
    /// the saved-register epilogue, and the call result stays in its EABI f1
    /// home while the member load uses f0.
    pub(crate) fn try_float_call_guard_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let Some(plan) = recognize(function, &self.call_return_types) else {
            return Ok(false);
        };
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::OrRecord {
                a: 31,
                s: 3,
                b: 3,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 20,
            },
        ]);
        self.load_float_constant(1, plan.argument);
        self.record_relocation(RelocationKind::Rel24, plan.call);
        self.output.instructions.extend([
            Instruction::BranchAndLink {
                target: plan.call.into(),
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 31,
                offset: plan.member_offset,
            },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 13,
            },
            Instruction::load_immediate(3, plan.first_result),
            Instruction::Branch { target: 21 },
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: plan.word_offset,
            },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 32 - plan.shift,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 18,
            },
            Instruction::load_immediate(3, plan.bit_result),
            Instruction::Branch { target: 21 },
            Instruction::load_immediate(3, plan.clear_result),
            Instruction::Branch { target: 21 },
            Instruction::load_immediate(3, plan.null_result),
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        // This body's file-scope initializer strings are numbered before its
        // six control-flow labels; move only the pool constant past those
        // labels instead of applying the ordinary pre-function float bump.
        self.output.constant_number_adjust += 6;
        Ok(true)
    }
}

fn recognize<'a>(
    function: &'a Function,
    call_return_types: &std::collections::HashMap<String, Type>,
) -> Option<Plan<'a>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        parameter.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !matches!(
            function.return_type,
            Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::UnsignedInt
        )
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(guard),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if guard != &parameter.name || !else_body.is_empty() {
        return None;
    }
    let [Statement::Return(Some(result))] = then_body.as_slice() else {
        return None;
    };
    let null_result = literal(function.return_expression.as_ref()?)?;
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = result
    else {
        return None;
    };
    let first_result = literal(when_true)?;
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    let member_offset = float_member(left, &parameter.name)?;
    let Expression::Call {
        name: call,
        arguments: call_arguments,
    } = right.as_ref()
    else {
        return None;
    };
    if call_return_types.get(call) != Some(&Type::Float) {
        return None;
    }
    let [Expression::FloatLiteral(argument)] = call_arguments.as_slice() else {
        return None;
    };
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = when_false.as_ref()
    else {
        return None;
    };
    let bit_result = literal(when_true)?;
    let clear_result = literal(when_false)?;
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    if constant_value(right) != Some(1) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    let shift = u8::try_from(constant_value(right)?).ok()?;
    if shift == 0 || shift >= 32 {
        return None;
    }
    let word_offset = word_member(left, &parameter.name)?;
    Some(Plan {
        member_offset,
        call,
        argument: *argument as f32,
        first_result,
        word_offset,
        shift,
        bit_result,
        clear_result,
        null_result,
    })
}

fn literal(expression: &Expression) -> Option<i16> {
    i16::try_from(constant_value(expression)?).ok()
}

fn float_member(expression: &Expression, base_name: &str) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == base_name)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn word_member(expression: &Expression, base_name: &str) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    if member_type.width() != 32
        || !matches!(base.as_ref(), Expression::Variable(name) if name == base_name)
    {
        return None;
    }
    i16::try_from(*offset).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ConditionalOrigin, Parameter};

    fn conditional(condition: Expression, when_true: i64, when_false: Expression) -> Expression {
        Expression::Conditional {
            condition: Box::new(condition),
            when_true: Box::new(Expression::IntegerLiteral(when_true)),
            when_false: Box::new(when_false),
            origin: ConditionalOrigin::IfReturns,
        }
    }

    #[test]
    fn recognizes_float_call_then_single_bit_return_chain() {
        let bit = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 0,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                }),
                right: Box::new(Expression::IntegerLiteral(25)),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let result = conditional(
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 28,
                    member_type: Type::Float,
                    index_stride: None,
                }),
                right: Box::new(Expression::Call {
                    name: "wave".into(),
                    arguments: vec![Expression::FloatLiteral(0.75)],
                }),
            },
            1,
            conditional(bit, 0, Expression::IntegerLiteral(1)),
        );
        let function = Function {
            return_type: Type::UnsignedChar,
            name: "classify".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 32 },
                name: "object".into(),
            }],
            locals: Vec::new(),
            statements: vec![Statement::If {
                condition: Expression::Variable("object".into()),
                then_body: vec![Statement::Return(Some(result))],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(1)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let returns = std::collections::HashMap::from([("wave".into(), Type::Float)]);
        let plan = recognize(&function, &returns).expect("the semantic family should match");
        assert_eq!(plan.member_offset, 28);
        assert_eq!(plan.word_offset, 0);
        assert_eq!(plan.shift, 25);
        assert_eq!(
            (
                plan.first_result,
                plan.bit_result,
                plan.clear_result,
                plan.null_result
            ),
            (1, 0, 1, 1)
        );
    }
}
