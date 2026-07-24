//! Call-heavy counted loops with two loop-carried scalar homes.
//!
//! The `DummyLen` family keeps a shift and iteration count in callee-saved
//! registers while a transient result flows through several calls.  The result
//! never crosses a back-edge call, so it remains in the result/scratch lane.

#[allow(unused_imports)]
use super::*;

struct CallLiveCounterLoop<'a> {
    tick: &'a str,
    seed: &'a str,
    random: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn add_one(expression: &Expression, expected: &str) -> bool {
    matches!(expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if variable(left, expected) && constant_value(right) == Some(1))
}

fn call_without_arguments(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Call { name, arguments } if arguments.is_empty() => Some(name),
        _ => None,
    }
}

fn masked_call_plus_one(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right) != Some(1) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    (constant_value(right) == Some(0x1f))
        .then(|| call_without_arguments(left))
        .flatten()
}

fn comparison(
    expression: &Expression,
    operator: BinaryOperator,
    name: &str,
    constant: i64,
) -> bool {
    matches!(expression,
        Expression::Binary {
            operator: actual,
            left,
            right,
        } if *actual == operator && variable(left, name) && constant_value(right) == Some(constant))
}

fn classify(function: &Function) -> Option<CallLiveCounterLoop<'_>> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.locals.len() != 3
    {
        return None;
    }
    let [shift, iteration, result] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(shift.declared_type, Type::Int | Type::UnsignedInt)
        || !matches!(iteration.declared_type, Type::Int | Type::UnsignedInt)
        || !matches!(result.declared_type, Type::Int | Type::UnsignedInt)
        || constant_value(shift.initializer.as_ref()?) != Some(1)
        || constant_value(iteration.initializer.as_ref()?) != Some(0)
        || result.initializer.is_some()
        || function.locals.iter().any(|local| {
            local.array_length.is_some() || local.is_static || local.is_volatile
        })
    {
        return None;
    }

    let [Statement::Expression(Expression::Call {
        name: seed,
        arguments: initial_seed_arguments,
    }), Statement::Assign {
        name: initial_result,
        value: initial_value,
    }, Statement::Loop {
        kind: LoopKind::For,
        initializer: None,
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [initial_tick] = initial_seed_arguments.as_slice() else {
        return None;
    };
    let tick = call_without_arguments(initial_tick)?;
    let random = masked_call_plus_one(initial_value)?;
    if initial_result != &result.name
        || !matches!(condition,
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            } if comparison(left, BinaryOperator::Less, &result.name, 4)
                && comparison(right, BinaryOperator::Less, &iteration.name, 10))
        || !matches!(step,
            Expression::Assign { target, value }
                if variable(target, &iteration.name)
                    && add_one(value, &iteration.name))
    {
        return None;
    }

    let [Statement::Assign {
        name: shifted_result,
        value: shifted_value,
    }, Statement::If {
        condition: shift_condition,
        then_body,
        else_body,
    }, Statement::Expression(Expression::Call {
        name: body_seed,
        arguments: body_seed_arguments,
    }), Statement::Assign {
        name: body_result,
        value: body_value,
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: shifted_tick,
        right: shift_operand,
    } = shifted_value
    else {
        return None;
    };
    let [Statement::Assign {
        name: reset_shift,
        value: reset_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [seed_result] = body_seed_arguments.as_slice() else {
        return None;
    };
    if shifted_result != &result.name
        || call_without_arguments(shifted_tick) != Some(tick)
        || !variable(shift_operand, &shift.name)
        || !else_body.is_empty()
        || !matches!(shift_condition,
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Assign { target, value }
                if variable(target, &shift.name) && add_one(value, &shift.name))
                && constant_value(right) == Some(16))
        || reset_shift != &shift.name
        || constant_value(reset_value) != Some(1)
        || body_seed != seed
        || !variable(seed_result, &result.name)
        || body_result != &result.name
        || masked_call_plus_one(body_value) != Some(random)
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            }) if comparison(condition, BinaryOperator::Less, &result.name, 4)
                && constant_value(when_true) == Some(4)
                && variable(when_false, &result.name))
    {
        return None;
    }

    Some(CallLiveCounterLoop {
        tick,
        seed,
        random,
    })
}

impl Generator {
    pub(crate) fn try_call_live_counter_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let shift = self.fresh_virtual_general_preferring(31);
        let iteration = self.fresh_virtual_general_preferring(30);
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![shift, iteration];
        self.output.pre_scheduled = true;
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => self.output.instructions.extend([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
                Instruction::StoreMultipleWord { s: iteration, a: 1, offset: 8 },
                Instruction::load_immediate(shift, 1),
                Instruction::load_immediate(iteration, 0),
            ]),
            FrameConvention::Predecrement => self.output.instructions.extend([
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 20 },
                Instruction::StoreWord { s: shift, a: 1, offset: 12 },
                Instruction::load_immediate(shift, 1),
                Instruction::StoreWord { s: iteration, a: 1, offset: 8 },
                Instruction::load_immediate(iteration, 0),
            ]),
        }

        for name in [shape.tick, shape.seed, shape.random] {
            self.record_relocation(RelocationKind::Rel24, name);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: name.to_string(),
            });
        }
        self.output.instructions.push(Instruction::ClearLeftImmediate {
            a: 3,
            s: 3,
            clear: 27,
        });
        let result = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => 3,
            FrameConvention::Predecrement => 0,
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: result,
            a: 3,
            immediate: 1,
        });
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output.instructions.push(Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: 0,
            });
        }
        let body = self.output.instructions.len();
        self.record_relocation(RelocationKind::Rel24, shape.tick);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.tick.to_string(),
        });
        self.output.instructions.extend([
            Instruction::ShiftLeftWord { a: 3, s: 3, b: shift },
            Instruction::AddImmediate { d: shift, a: shift, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: shift, immediate: 16 },
        ]);
        let keep_shift = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 0,
        });
        self.output.instructions.push(Instruction::load_immediate(shift, 1));
        let after_reset = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[keep_shift]
        {
            *target = after_reset;
        }
        for name in [shape.seed, shape.random] {
            self.record_relocation(RelocationKind::Rel24, name);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: name.to_string(),
            });
        }
        self.output.instructions.push(Instruction::ClearLeftImmediate {
            a: 3,
            s: 3,
            clear: 27,
        });
        let fill_mask_latency = self.behavior.power_pc_7400_scheduling_enabled();
        if fill_mask_latency {
            self.output.instructions.push(Instruction::AddImmediate {
                d: iteration,
                a: iteration,
                immediate: 1,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: result,
            a: 3,
            immediate: 1,
        });
        if !fill_mask_latency {
            self.output.instructions.push(Instruction::AddImmediate {
                d: iteration,
                a: iteration,
                immediate: 1,
            });
        }
        let test = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = test;
        }
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: result,
            immediate: 4,
        });
        let exit = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 0,
        });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate {
            a: iteration,
            immediate: 10,
        });
        self.output.instructions.push(Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 0,
            target: body,
        });
        let terminal = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[exit]
        {
            *target = terminal;
        }
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: result,
            immediate: 4,
        });
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                let keep = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 0,
                    target: 0,
                });
                self.output.instructions.push(Instruction::load_immediate(3, 4));
                let done = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[keep]
                {
                    *target = done;
                }
                self.output.instructions.extend([
                    Instruction::LoadWord { d: 0, a: 1, offset: 20 },
                    Instruction::LoadMultipleWord { d: iteration, a: 1, offset: 8 },
                    Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::BranchToLinkRegister,
                ]);
            }
            FrameConvention::Predecrement => {
                self.output.instructions.push(Instruction::load_immediate(3, 4));
                let selected = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 0,
                    target: selected + 2,
                });
                self.output.instructions.push(Instruction::move_register(3, result));
                self.output.instructions.extend([
                    Instruction::LoadWord { d: 0, a: 1, offset: 20 },
                    Instruction::LoadWord { d: shift, a: 1, offset: 12 },
                    Instruction::LoadWord { d: iteration, a: 1, offset: 8 },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
                    Instruction::BranchToLinkRegister,
                ]);
            }
        }
        self.output.anonymous_label_bump += 10;
        Ok(true)
    }
}
