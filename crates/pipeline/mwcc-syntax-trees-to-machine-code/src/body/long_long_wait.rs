//! Volatile 64-bit timer waits.
//!
//! This is the first non-trivial long-long body lowered outside the scalar
//! pair-return path.  Recognition is semantic (local roles and expression
//! relationships), while emission owns the measured pair homes, frame slots,
//! and call schedule in one place.

use super::*;

struct WaitPlan<'a> {
    clock: ClockRead<'a>,
    time_call: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn classify(function: &Function) -> Option<WaitPlan<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [duration, end, current, difference] = function.locals.as_slice() else {
        return None;
    };
    if duration.declared_type != Type::LongLong
        || duration.is_volatile
        || duration.is_static
        || duration.array_length.is_some()
        || end.declared_type != Type::LongLong
        || end.is_volatile
        || end.is_static
        || end.array_length.is_some()
        || current.declared_type != Type::LongLong
        || !current.is_volatile
        || current.initializer.is_some()
        || current.is_static
        || current.array_length.is_some()
        || difference.declared_type != Type::Int
        || !difference.is_volatile
        || difference.initializer.is_some()
        || difference.is_static
        || difference.array_length.is_some()
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: seconds,
        right: scale,
    } = duration.initializer.as_ref()?
    else {
        return None;
    };
    if function
        .parameters
        .iter()
        .filter(|parameter| parameter.parameter_type == Type::Float)
        .count()
        != 1
        || !function.parameters.iter().any(|parameter| {
            parameter.parameter_type == Type::Float && variable(seconds, &parameter.name)
        })
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: clock,
        right: divisor,
    } = scale.as_ref()
    else {
        return None;
    };
    if constant_value(divisor) != Some(4) {
        return None;
    }
    let clock = unsigned_word_clock(clock)?;

    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: first_time,
        right: duration_use,
    } = end.initializer.as_ref()?
    else {
        return None;
    };
    let Expression::Call {
        name: time_call,
        arguments: first_arguments,
    } = first_time.as_ref()
    else {
        return None;
    };
    if !first_arguments.is_empty() || !variable(duration_use, &duration.name) {
        return None;
    }

    let [Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: condition_value,
        right: condition_zero,
    } = condition
    else {
        return None;
    };
    let [Statement::Assign {
        name: current_target,
        value:
            Expression::Call {
                name: loop_call,
                arguments: loop_arguments,
            },
    }, Statement::Assign {
        name: difference_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: current_use,
                right: end_use,
            },
    }] = body.as_slice()
    else {
        return None;
    };
    if current_target != &current.name
        || difference_target != &difference.name
        || time_call != loop_call
        || !loop_arguments.is_empty()
        || !variable(current_use, &current.name)
        || !variable(end_use, &end.name)
        || !variable(condition_value, &difference.name)
        || constant_value(condition_zero) != Some(0)
    {
        return None;
    }

    Some(WaitPlan { clock, time_call })
}

impl Generator {
    pub(crate) fn try_volatile_long_long_wait(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if !matches!(
            self.call_return_types.get(plan.time_call),
            Some(Type::LongLong | Type::UnsignedLongLong)
        ) {
            return Ok(false);
        }
        if self.behavior.long_long_timer_style != LongLongTimerStyle::MainlinePair
            || !self.supports_unsigned_word_clock(plan.clock)
            {
                return Ok(false);
            }

        self.output.pre_scheduled = true;
        self.output.has_conversion = true;
        // The optimizer consumes seven internal labels for the conversion,
        // pair-live range, and do/while graph before numbering this function's
        // pooled bias double (measured across the 2.4.x mainline builds).
        self.output.anonymous_label_bump += 7;
        // Deferred compilation analyzes this source-first transaction before a
        // later body that may be emitted ahead of it. Its conversion label,
        // seven internal labels, and one pool slot then prefix that reversed
        // head's ordinal block while this function keeps its own pool schedule.
        self.output.deferred_source_prefix_bump = 9;
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30];
        self.epilogue_lr_before_gprs = true;

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_unsigned_word_clock_high(plan.clock, 3);
        if !self.behavior.float_cast_value_store_first {
            self.load_double_constant(2, 0x4330_0000_0000_0000);
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 44,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 40,
        });
        self.emit_unsigned_word_clock_load(plan.clock, 3);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 3,
                shift: 2,
            });
        if self.behavior.float_cast_value_store_first {
            self.load_double_constant(2, 0x4330_0000_0000_0000);
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 });
        self.record_relocation(RelocationKind::Rel24, "__cvt_dbl_usll");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__cvt_dbl_usll".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, plan.time_call);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.time_call.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::AddCarrying { d: 30, a: 30, b: 4 });
        self.output
            .instructions
            .push(Instruction::AddExtended { d: 31, a: 31, b: 3 });

        let loop_body = self.fresh_label();
        self.bind_label(loop_body);
        self.record_relocation(RelocationKind::Rel24, plan.time_call);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.time_call.to_string(),
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying { d: 0, a: 30, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, loop_body);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
