//! Integer-quantized float arithmetic with an optional second scale.
//!
//! MWCC shares one signed-int-to-double bias across three conversion images and
//! schedules the independent unsigned range test before the first global load.
//! Treating the two source locals independently loses that frame layout and the
//! cross-expression schedule.

#[allow(unused_imports)]
use super::*;

struct FloatMember<'a> {
    base: &'a str,
    offset: i16,
}

struct ConditionalFloatRequantize<'a> {
    integer: &'a str,
    selector: &'a str,
    multiplier: &'a str,
    global: &'a str,
    first_factor_offset: i16,
    first_addend_offset: i16,
    conditional_factor_offset: i16,
    range_start: i16,
    range_width: u16,
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

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn int_cast_multiply(expression: &Expression, left: &str, right: &str) -> bool {
    matches!(
        expression,
        Expression::Cast {
            target_type: Type::Int,
            operand,
        } if matches!(
            operand.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: multiply_left,
                right: multiply_right,
            } if variable(multiply_left, left) && variable(multiply_right, right)
        )
    )
}

fn classify(function: &Function) -> Option<ConditionalFloatRequantize<'_>> {
    if function.return_type != Type::Float
        || !function.guards.is_empty()
        || function_makes_call(function)
        || function.asm_body.is_some()
    {
        return None;
    }
    let [integer, selector, multiplier] = function.parameters.as_slice() else {
        return None;
    };
    if integer.parameter_type != Type::Int
        || selector.parameter_type != Type::Int
        || multiplier.parameter_type != Type::Float
    {
        return None;
    }
    let [first, result] = function.locals.as_slice() else {
        return None;
    };
    if first.declared_type != Type::Int
        || result.declared_type != Type::Float
        || first.is_volatile
        || result.is_volatile
        || first.array_length.is_some()
        || result.array_length.is_some()
        || first.is_static
        || result.is_static
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: first_product,
        right: first_addend,
    } = first.initializer.as_ref()?
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: first_input,
        right: first_factor,
    } = first_product.as_ref()
    else {
        return None;
    };
    if !variable(first_input, &integer.name)
        || !int_cast_multiply(
            result.initializer.as_ref()?,
            &first.name,
            &multiplier.name,
        )
    {
        return None;
    }
    let first_factor = float_member(first_factor)?;
    let first_addend = float_member(first_addend)?;
    if first_factor.base != first_addend.base {
        return None;
    }
    let [Statement::If {
        condition,
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
        operator: BinaryOperator::LessEqual,
        left: range_value,
        right: range_width,
    } = condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: unsigned_selector,
        right: range_start,
    } = range_value.as_ref()
    else {
        return None;
    };
    if !matches!(
        unsigned_selector.as_ref(),
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } if variable(operand, &selector.name)
    ) {
        return None;
    }
    let range_start = constant_value(range_start).and_then(|value| i16::try_from(value).ok())?;
    let range_width = constant_value(range_width).and_then(|value| u16::try_from(value).ok())?;
    let [Statement::Assign {
        name: assigned_result,
        value: conditional_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Int,
        operand: conditional_product,
    } = conditional_value
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: prior_result,
        right: conditional_factor,
    } = conditional_product.as_ref()
    else {
        return None;
    };
    let conditional_factor = float_member(conditional_factor)?;
    if assigned_result != &result.name
        || !variable(prior_result, &result.name)
        || conditional_factor.base != first_factor.base
        || !function
            .return_expression
            .as_ref()
            .is_some_and(|expression| variable(expression, &result.name))
    {
        return None;
    }
    Some(ConditionalFloatRequantize {
        integer: &integer.name,
        selector: &selector.name,
        multiplier: &multiplier.name,
        global: first_factor.base,
        first_factor_offset: first_factor.offset,
        first_addend_offset: first_addend.offset,
        conditional_factor_offset: conditional_factor.offset,
        range_start,
        range_width,
    })
}

impl Generator {
    pub(crate) fn try_conditional_float_requantize(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.behavior.legacy_float_cast_schedule
            || self.general_register_of(shape.integer)? != 3
            || self.general_register_of(shape.selector)? != 4
            || self.float_register_of(shape.multiplier)? != 1
        {
            return Ok(false);
        }
        self.frame_size = 64;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 3,
                immediate: 0x8000,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 60,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: shape.range_start.wrapping_neg(),
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.global);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: shape.range_width,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 56,
        });
        self.load_double_constant(4, 0x4330_0000_8000_0000);
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 3,
            a: 1,
            offset: 56,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: 5,
            offset: shape.first_factor_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 3, a: 3, b: 4 });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 5,
            offset: shape.first_addend_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: 0,
                a: 3,
                c: 2,
                b: 0,
            });
        self.emit_float_requantize_image(0, 48, 52, 44, 40, 0);
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: 0,
                a: 0,
                c: 1,
            });
        self.emit_float_requantize_image(0, 32, 36, 28, 24, 1);
        let branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 0,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 5,
            offset: shape.conditional_factor_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: 0,
                a: 1,
                c: 0,
            });
        self.emit_float_requantize_image(0, 24, 28, 36, 32, 1);
        let epilogue = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch]
        {
            *target = epilogue;
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_float_requantize_image(
        &mut self,
        source: u8,
        converted_offset: i16,
        converted_word_offset: i16,
        biased_word_offset: i16,
        biased_high_offset: i16,
        destination: u8,
    ) {
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero {
                d: source,
                b: source,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: source,
                a: 1,
                offset: converted_offset,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: converted_word_offset,
        });
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0x8000,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: biased_word_offset,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: biased_high_offset,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: biased_high_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: destination,
                a: 0,
                b: 4,
            });
    }
}
