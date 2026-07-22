//! Dolphin CARD bit-reversal loop recognition and emission.
//!
//! The SDK implementation is copied into many projects. MWCC maps its fixed
//! 32-iteration loop to CTR and chooses an unroll factor from the `,p`/`,s`
//! optimization objective. Keeping the semantic matcher here prevents the
//! shared statement emitter from acquiring an SDK-specific control-flow case.

#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
struct BitReverseLoop {
    data: String,
}

impl Generator {
    pub(crate) fn try_bit_reverse_loop(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O4
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(shape) = recognize_bit_reverse_loop(function) else {
            return Ok(false);
        };
        let Some(data_register) = self.lookup_general(&shape.data) else {
            return Ok(false);
        };
        if data_register != 3 {
            return Ok(false);
        }

        let policy = super::policy::IntegerLoopPolicy::resolve(self.behavior.integer_loop_style);
        let unroll = match self.behavior.optimization_goal {
            mwcc_versions::OptimizationGoal::Size => 1,
            mwcc_versions::OptimizationGoal::Performance if policy.dependency_first => 4,
            mwcc_versions::OptimizationGoal::Performance => 2,
        };

        const MASK: u8 = 5;
        const TOP_BIT: u8 = 6;
        const WORK: u8 = 7;
        const INDEX: u8 = 8;
        const LOW_COUNT: u8 = 9;
        const HIGH_SHIFT: u8 = 10;

        self.output
            .instructions
            .push(Instruction::load_immediate(0, 32 / unroll));
        if policy.scaffold_after_ctr {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 0 });
        }
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: TOP_BIT,
                s: data_register,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(LOW_COUNT, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(HIGH_SHIFT, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(WORK, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(INDEX, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(MASK, 1));
        if !policy.scaffold_after_ctr {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 0 });
        }

        let loop_body = self.fresh_label();
        self.bind_label(loop_body);
        for _ in 0..unroll {
            self.emit_bit_reverse_iteration(data_register, policy.dependency_first);
        }
        self.emit_branch_conditional_to(16, 0, loop_body);
        self.output
            .instructions
            .push(Instruction::move_register(3, WORK));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        // A source `for` consumes five internal ordinals; its two nested `if`
        // statements consume two each. Unrolling changes machine labels, not
        // this source-level accounting.
        self.output.anonymous_label_bump = 9;
        Ok(true)
    }

    fn emit_bit_reverse_iteration(&mut self, data: u8, dependency_first: bool) {
        const MASK: u8 = 5;
        const TOP_BIT: u8 = 6;
        const WORK: u8 = 7;
        const INDEX: u8 = 8;
        const LOW_COUNT: u8 = 9;
        const HIGH_SHIFT: u8 = 10;

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: INDEX,
                immediate: 15,
            });
        let low_half = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, low_half); // ble
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: INDEX,
                immediate: 31,
            });
        let ordinary_high = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, ordinary_high); // bne
        self.output.instructions.push(Instruction::Or {
            a: WORK,
            s: WORK,
            b: TOP_BIT,
        });
        let iteration_done = self.fresh_label();
        self.emit_branch_to(iteration_done);

        self.bind_label(ordinary_high);
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: 0,
            s: MASK,
            b: INDEX,
        });
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: data,
            b: 0,
        });
        self.output.instructions.push(Instruction::ShiftRightWord {
            a: 0,
            s: 0,
            b: HIGH_SHIFT,
        });
        if dependency_first {
            self.output.instructions.push(Instruction::Or {
                a: WORK,
                s: WORK,
                b: 0,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: HIGH_SHIFT,
            a: HIGH_SHIFT,
            immediate: 2,
        });
        if !dependency_first {
            self.output.instructions.push(Instruction::Or {
                a: WORK,
                s: WORK,
                b: 0,
            });
        }
        self.emit_branch_to(iteration_done);

        self.bind_label(low_half);
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: 4,
            s: MASK,
            b: INDEX,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: INDEX,
                immediate: 31,
            });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: 0,
            a: LOW_COUNT,
            b: 0,
        });
        if !dependency_first {
            self.output.instructions.push(Instruction::AddImmediate {
                d: LOW_COUNT,
                a: LOW_COUNT,
                immediate: 1,
            });
        }
        self.output.instructions.push(Instruction::And {
            a: 4,
            s: data,
            b: 4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::Or {
            a: WORK,
            s: WORK,
            b: 0,
        });
        if dependency_first {
            self.output.instructions.push(Instruction::AddImmediate {
                d: LOW_COUNT,
                a: LOW_COUNT,
                immediate: 1,
            });
        }

        self.bind_label(iteration_done);
        self.output.instructions.push(Instruction::AddImmediate {
            d: INDEX,
            a: INDEX,
            immediate: 1,
        });
    }
}

fn recognize_bit_reverse_loop(function: &Function) -> Option<BitReverseLoop> {
    if function.return_type != Type::UnsignedInt
        || !function.guards.is_empty()
        || function.parameters.len() != 1
        || function.locals.len() != 4
        || function_makes_call(function)
    {
        return None;
    }
    let parameter = &function.parameters[0];
    if parameter.parameter_type != Type::UnsignedInt {
        return None;
    }
    let Expression::Variable(work) = function.return_expression.as_ref()? else {
        return None;
    };
    let work_local = function.locals.iter().find(|local| local.name == *work)?;
    if work_local.initializer.is_some() || !is_plain_unsigned_local(work_local) {
        return None;
    }
    let low_count = function.locals.iter().find(|local| {
        local
            .initializer
            .as_ref()
            .is_some_and(|value| is_constant(value, 0))
    })?;
    let high_shift = function.locals.iter().find(|local| {
        local
            .initializer
            .as_ref()
            .is_some_and(|value| is_constant(value, 1))
    })?;
    if !is_plain_unsigned_local(low_count) || !is_plain_unsigned_local(high_shift) {
        return None;
    }
    let index = function.locals.iter().find(|local| {
        local.name != *work
            && local.name != low_count.name
            && local.name != high_shift.name
            && local.initializer.is_none()
            && is_plain_unsigned_local(local)
    })?;

    let [zero_work, loop_statement] = function.statements.as_slice() else {
        return None;
    };
    if !is_statement_assignment(zero_work, work, 0) {
        return None;
    }
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = loop_statement
    else {
        return None;
    };
    if !is_expression_assignment(initializer, &index.name, 0)
        || !is_comparison(condition, BinaryOperator::Less, &index.name, 32)
        || !is_expression_increment(step, &index.name, 1)
    {
        return None;
    }
    let [Statement::If {
        condition: high_condition,
        then_body: high_body,
        else_body: low_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if !is_comparison(high_condition, BinaryOperator::Greater, &index.name, 15) {
        return None;
    }
    let [Statement::If {
        condition: top_condition,
        then_body: top_body,
        else_body: ordinary_high_body,
    }] = high_body.as_slice()
    else {
        return None;
    };
    if !is_comparison(top_condition, BinaryOperator::Equal, &index.name, 31) {
        return None;
    }
    let [top_update] = top_body.as_slice() else {
        return None;
    };
    if !statement_or_rhs(top_update, work).is_some_and(|rhs| is_top_bit(rhs, &parameter.name)) {
        return None;
    }
    let [high_update, high_step] = ordinary_high_body.as_slice() else {
        return None;
    };
    if !statement_or_rhs(high_update, work)
        .is_some_and(|rhs| is_high_half_bit(rhs, &parameter.name, &index.name, &high_shift.name))
        || !is_statement_increment(high_step, &high_shift.name, 2)
    {
        return None;
    }
    let [low_update, low_step] = low_body.as_slice() else {
        return None;
    };
    if !statement_or_rhs(low_update, work)
        .is_some_and(|rhs| is_low_half_bit(rhs, &parameter.name, &index.name, &low_count.name))
        || !is_statement_increment(low_step, &low_count.name, 1)
    {
        return None;
    }

    Some(BitReverseLoop {
        data: parameter.name.clone(),
    })
}

fn is_plain_unsigned_local(local: &LocalDeclaration) -> bool {
    local.declared_type == Type::UnsignedInt
        && !local.is_static
        && !local.is_volatile
        && local.array_length.is_none()
}

fn is_statement_assignment(statement: &Statement, name: &str, value: i64) -> bool {
    matches!(statement, Statement::Assign { name: found, value: expression }
        if found == name && is_constant(expression, value))
}

fn is_expression_assignment(expression: &Expression, name: &str, value: i64) -> bool {
    matches!(strip_casts(expression), Expression::Assign { target, value: assigned }
        if is_variable(target, name) && is_constant(assigned, value))
}

fn is_expression_increment(expression: &Expression, name: &str, amount: i64) -> bool {
    matches!(strip_casts(expression), Expression::Assign { target, value }
        if is_variable(target, name) && is_increment_value(value, name, amount))
}

fn is_statement_increment(statement: &Statement, name: &str, amount: i64) -> bool {
    matches!(statement, Statement::Assign { name: found, value }
        if found == name && is_increment_value(value, name, amount))
}

fn is_increment_value(expression: &Expression, name: &str, amount: i64) -> bool {
    matches!(strip_casts(expression), Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if is_variable(left, name) && is_constant(right, amount))
}

fn is_comparison(
    expression: &Expression,
    expected: BinaryOperator,
    variable: &str,
    constant: i64,
) -> bool {
    matches!(strip_casts(expression), Expression::Binary { operator, left, right }
        if *operator == expected && is_variable(left, variable) && is_constant(right, constant))
}

fn statement_or_rhs<'a>(statement: &'a Statement, accumulator: &str) -> Option<&'a Expression> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    if name != accumulator {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = strip_casts(value)
    else {
        return None;
    };
    is_variable(left, accumulator).then_some(strip_casts(right))
}

fn is_top_bit(expression: &Expression, data: &str) -> bool {
    let expression = match strip_casts(expression) {
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } if is_constant(right, 1) => strip_casts(left),
        other => other,
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left,
        right,
    } = expression
    else {
        return false;
    };
    is_constant(right, 31) && is_masked_bit(left, data, BitIndex::Constant(31))
}

fn is_high_half_bit(expression: &Expression, data: &str, index: &str, shift: &str) -> bool {
    matches!(strip_casts(expression), Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left,
        right,
    } if is_variable(right, shift) && is_masked_bit(left, data, BitIndex::Variable(index)))
}

fn is_low_half_bit(expression: &Expression, data: &str, index: &str, count: &str) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left,
        right,
    } = strip_casts(expression)
    else {
        return false;
    };
    if !is_masked_bit(left, data, BitIndex::Variable(index)) {
        return false;
    }
    matches!(strip_casts(right), Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: first,
        right: last,
    } if is_variable(last, count)
        && matches!(strip_casts(first), Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: thirty_one,
            right: loop_index,
        } if is_constant(thirty_one, 31) && is_variable(loop_index, index)))
}

#[derive(Clone, Copy)]
enum BitIndex<'a> {
    Constant(i64),
    Variable(&'a str),
}

fn is_masked_bit(expression: &Expression, data: &str, index: BitIndex<'_>) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = strip_casts(expression)
    else {
        return false;
    };
    if !is_variable(left, data) {
        return false;
    }
    if let BitIndex::Constant(bit) = index {
        if (0..32).contains(&bit) && is_constant(right, 1i64 << bit) {
            return true;
        }
    }
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: one,
        right: amount,
    } = strip_casts(right)
    else {
        return false;
    };
    if !is_constant(one, 1) {
        return false;
    }
    match index {
        BitIndex::Constant(value) => is_constant(amount, value),
        BitIndex::Variable(name) => is_variable(amount, name),
    }
}

fn strip_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(strip_casts(expression), Expression::Variable(found) if found == name)
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(strip_casts(expression)) == Some(expected)
}
