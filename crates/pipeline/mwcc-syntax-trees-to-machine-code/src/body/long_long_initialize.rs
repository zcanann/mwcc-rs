//! Initialization of adjacent 64-bit timer fields and their float period.
//!
//! The source transaction stores a widened clock, divides that pair through
//! the EABI runtime helper, converts the original signed pair to float, and
//! stores its reciprocal. Recognition keeps those value relationships explicit;
//! emission owns the measured pair/register schedule.

use super::*;

struct InitializePlan<'a> {
    this: &'a str,
    clock: ClockRead<'a>,
    frequency_offset: i16,
    scaled_offset: i16,
    period_offset: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn member<'a>(expression: &'a Expression, base_name: &str, member_type: Type) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        member_type: actual_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (*actual_type == member_type && variable(base, base_name))
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn classify(function: &Function) -> Option<InitializePlan<'_>> {
    if !matches!(function.return_type, Type::UnsignedChar)
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [this] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(this.parameter_type, Type::StructPointer { .. }) {
        return None;
    }
    let [Statement::Store {
        target: frequency_target,
        value: frequency_value,
    }, Statement::Store {
        target: scaled_target,
        value: scaled_value,
    }, Statement::Store {
        target: period_target,
        value: period_value,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let frequency_offset = member(frequency_target, &this.name, Type::LongLong)?;
    let scaled_offset = member(scaled_target, &this.name, Type::LongLong)?;
    let period_offset = member(period_target, &this.name, Type::Float)?;
    if scaled_offset != frequency_offset.checked_add(8)?
        || period_offset != scaled_offset.checked_add(8)?
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: clock,
        right: clock_divisor,
    } = frequency_value
    else {
        return None;
    };
    if constant_value(clock_divisor) != Some(4) {
        return None;
    }
    let clock = unsigned_word_clock(clock)?;

    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: scaled_numerator,
        right: scaled_divisor,
    } = scaled_value
    else {
        return None;
    };
    if member(scaled_numerator, &this.name, Type::LongLong) != Some(frequency_offset)
        || constant_value(scaled_divisor) != Some(1_000_000)
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: reciprocal_one,
        right: reciprocal_divisor,
    } = period_value
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Float,
        operand: converted_frequency,
    } = reciprocal_divisor.as_ref()
    else {
        return None;
    };
    if !matches!(reciprocal_one.as_ref(), Expression::FloatLiteral(value) if *value == 1.0)
        || member(converted_frequency, &this.name, Type::LongLong) != Some(frequency_offset)
    {
        return None;
    }

    Some(InitializePlan {
        this: &this.name,
        clock,
        frequency_offset,
        scaled_offset,
        period_offset,
    })
}

impl Generator {
    pub(crate) fn try_long_long_member_initialize(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.long_long_timer_style != LongLongTimerStyle::MainlinePair
            || self.lookup_general(plan.this) != Some(3)
            || !self.supports_unsigned_word_clock(plan.clock)
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.frame_size = 16;
        self.non_leaf = true;
        self.callee_saved = vec![31];

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 15));
        self.emit_unsigned_word_clock_high(plan.clock, 5);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 4,
            immediate: 16960,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.emit_unsigned_word_clock_load(plan.clock, 5);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 5,
                shift: 2,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: plan.frequency_offset + 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: plan.frequency_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: plan.frequency_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: plan.frequency_offset + 4,
        });
        self.record_relocation(RelocationKind::Rel24, "__div2i");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__div2i".to_string(),
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 31,
            offset: plan.scaled_offset + 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: plan.scaled_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: plan.frequency_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: plan.frequency_offset + 4,
        });
        self.record_relocation(RelocationKind::Rel24, "__cvt_sll_flt");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_sll_flt".to_string(),
        });
        self.load_float_constant(0, 1.0);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 0, a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 31,
                offset: plan.period_offset,
            });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
