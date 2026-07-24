//! Guarded table indexing through a quantized float ratio.
//!
//! This family needs one whole-body owner: MWCC shares the table address across
//! the conversion path, orders the three float constants by source occurrence,
//! and uses a frame slot for `fctiwz` extraction.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Clone, Debug)]
pub(crate) struct GuardedFloatTableIndexSummary {
    pub(crate) numerator: String,
    pub(crate) denominator: String,
    pub(crate) table: String,
    pub(crate) zero: f64,
    pub(crate) scale: f64,
    pub(crate) bias: f64,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn zero_table_element<'a>(
    expression: &'a Expression,
    return_type: Type,
) -> Option<&'a str> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::Variable(table) = base.as_ref() else {
        return None;
    };
    (return_type == Type::UnsignedShort && constant_value(index) == Some(0))
        .then_some(table)
        .map(String::as_str)
}

pub(crate) fn summarize_guarded_float_table_index(
    function: &Function,
) -> Option<GuardedFloatTableIndexSummary> {
    if function.return_type != Type::UnsignedShort
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || function.asm_body.is_some()
        || function_makes_call(function)
    {
        return None;
    }
    let [numerator, denominator] = function.parameters.as_slice() else {
        return None;
    };
    if numerator.parameter_type != Type::Float || denominator.parameter_type != Type::Float {
        return None;
    }
    let [guard] = function.guards.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: guard_left,
        right: guard_right,
    } = &guard.condition
    else {
        return None;
    };
    let zero = match (guard_left.as_ref(), guard_right.as_ref()) {
        (Expression::Variable(name), Expression::FloatLiteral(value))
            if name == &denominator.name =>
        {
            *value
        }
        (Expression::FloatLiteral(value), Expression::Variable(name))
            if name == &denominator.name =>
        {
            *value
        }
        _ => return None,
    };
    if zero != 0.0 {
        return None;
    }
    let table = zero_table_element(&guard.value, function.return_type)?;
    let Expression::Index {
        base: return_base,
        index: return_index,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    if !matches!(
        return_base.as_ref(),
        Expression::Variable(name) if name == table
    ) {
        return None;
    }
    let Expression::Cast {
        target_type: Type::Int,
        operand: index_value,
    } = return_index.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: scaled_ratio,
        right: bias,
    } = index_value.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: ratio,
        right: scale,
    } = scaled_ratio.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: ratio_numerator,
        right: ratio_denominator,
    } = ratio.as_ref()
    else {
        return None;
    };
    let (Expression::FloatLiteral(scale), Expression::FloatLiteral(bias)) =
        (scale.as_ref(), bias.as_ref())
    else {
        return None;
    };
    if !variable(ratio_numerator, &numerator.name)
        || !variable(ratio_denominator, &denominator.name)
        || !scale.is_finite()
        || !bias.is_finite()
    {
        return None;
    }
    Some(GuardedFloatTableIndexSummary {
        numerator: numerator.name.clone(),
        denominator: denominator.name.clone(),
        table: table.to_owned(),
        zero,
        scale: *scale,
        bias: *bias,
    })
}

impl Generator {
    pub(crate) fn try_guarded_float_table_index(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = summarize_guarded_float_table_index(function) else {
            return Ok(false);
        };
        if !self.behavior.legacy_float_cast_schedule
            || self.float_register_of(&shape.numerator)? != 1
            || self.float_register_of(&shape.denominator)? != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        // Pool order follows source traversal, not the later instruction
        // schedule: guard zero, additive bias, then multiplicative scale.
        let zero = self
            .output
            .intern_constant((shape.zero as f32).to_bits() as u64, 4);
        let bias = self
            .output
            .intern_constant((shape.bias as f32).to_bits() as u64, 4);
        let scale = self
            .output
            .intern_constant((shape.scale as f32).to_bits() as u64, 4);

        self.frame_size = 24;
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![shape.table.clone()];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            });
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(zero));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 2 });
        let conversion = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, conversion);

        self.emit_address_high(3, &shape.table);
        self.emit_address_low(3, &shape.table);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 3,
                offset: 0,
            });
        let epilogue = self.fresh_label();
        self.emit_branch_to(epilogue);

        self.bind_label(conversion);
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 0, a: 1, b: 2 });
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(scale));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            });
        self.emit_address_high(3, &shape.table);
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(bias));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 2,
                a: 0,
                offset: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &shape.table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 2, b: 0 });
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 3,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 3,
                a: 3,
                offset: 0,
            });

        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}
