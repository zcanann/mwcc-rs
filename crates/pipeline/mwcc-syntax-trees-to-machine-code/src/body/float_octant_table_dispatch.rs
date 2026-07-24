//! Eight-octant float dispatch around an inlined quantized-ratio table helper.
//!
//! The source is a nested sign/quadrant decision tree whose leaves call one
//! small helper. MWCC expands that helper at every leaf, shares the translation
//! unit's `.data` base across the whole function, and uses one conversion slot.
//! This owner validates both the caller tree and the summarized callee before
//! emitting that interprocedural schedule.

use super::*;
use mwcc_machine_code::{DataSectionDisplacement, RelocationTarget};

struct FloatOctantTableDispatch {
    table: String,
    zero: f64,
    scale: f64,
    bias: f64,
}

#[derive(Clone, Copy)]
enum Argument<'a> {
    Plain(&'a str),
    Negated(&'a str),
}

#[derive(Clone, Copy)]
enum ResultAdjustment {
    Direct,
    Add(i64),
    SubtractFrom(i64),
    Negate,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn negated_variable(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } if variable(operand, expected)
    )
}

fn argument_matches(expression: &Expression, expected: Argument<'_>) -> bool {
    match expected {
        Argument::Plain(name) => variable(expression, name),
        Argument::Negated(name) => negated_variable(expression, name),
    }
}

fn comparison(
    expression: &Expression,
    operator: BinaryOperator,
    left: Argument<'_>,
    right: Argument<'_>,
) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: found,
            left: found_left,
            right: found_right,
        } if *found == operator
            && argument_matches(found_left, left)
            && argument_matches(found_right, right)
    )
}

fn comparison_with_zero(
    expression: &Expression,
    operator: BinaryOperator,
    left: &str,
) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: found,
            left: found_left,
            right,
        } if *found == operator
            && variable(found_left, left)
            && matches!(right.as_ref(), Expression::FloatLiteral(value) if *value == 0.0)
    )
}

fn one_if(statement: &Statement) -> Option<(&Expression, &[Statement], &[Statement])> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    Some((condition, then_body, else_body))
}

fn call_matches(
    expression: &Expression,
    callee: &str,
    arguments: [Argument<'_>; 2],
) -> bool {
    matches!(
        expression,
        Expression::Call {
            name,
            arguments: found,
        } if name == callee
            && matches!(found.as_slice(), [first, second]
                if argument_matches(first, arguments[0])
                    && argument_matches(second, arguments[1]))
    )
}

fn leaf_matches(
    body: &[Statement],
    result: &str,
    callee: &str,
    arguments: [Argument<'_>; 2],
    adjustment: ResultAdjustment,
) -> bool {
    let [Statement::Assign { name, value }] = body else {
        return false;
    };
    if name != result {
        return false;
    }
    match adjustment {
        ResultAdjustment::Direct => call_matches(value, callee, arguments),
        ResultAdjustment::Add(constant) => matches!(
            value,
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if call_matches(left, callee, arguments)
                && constant_value(right) == Some(constant)
        ),
        ResultAdjustment::SubtractFrom(constant) => matches!(
            value,
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } if constant_value(left) == Some(constant)
                && call_matches(right, callee, arguments)
        ),
        ResultAdjustment::Negate => matches!(
            value,
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } if call_matches(operand, callee, arguments)
        ),
    }
}

fn direct_leaf_callee<'a>(body: &'a [Statement], result: &str) -> Option<&'a str> {
    let [Statement::Assign {
        name,
        value: Expression::Call { name: callee, .. },
    }] = body
    else {
        return None;
    };
    (name == result).then_some(callee.as_str())
}

fn empty_false_loop(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        } if body.is_empty() && constant_value(condition) == Some(0)
    )
}

fn classify(
    function: &Function,
    summaries: &crate::InlineSummaries,
) -> Option<FloatOctantTableDispatch> {
    if function.return_type != Type::UnsignedShort
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [x, y] = function.parameters.as_slice() else {
        return None;
    };
    if x.parameter_type != Type::Float || y.parameter_type != Type::Float {
        return None;
    }
    let Expression::Variable(result) = function.return_expression.as_ref()? else {
        return None;
    };
    if !function.locals.iter().any(|local| {
        local.name == *result
            && local.declared_type == Type::Int
            && local.array_length.is_none()
            && local.initializer.is_none()
            && !local.is_static
    }) || function.locals.iter().any(|local| {
        local.name != *result
            && (local.array_length.is_none()
                || local.initializer.is_some()
                || local.is_static
                || local.is_volatile)
    }) {
        return None;
    }
    let outer = match function.statements.as_slice() {
        [outer] => outer,
        [outer, trailing] if empty_false_loop(trailing) => outer,
        _ => return None,
    };
    let (outer_condition, upper_body, lower_body) = one_if(outer)?;
    if !comparison_with_zero(
        outer_condition,
        BinaryOperator::GreaterEqual,
        &y.name,
    ) {
        return None;
    }

    let [upper_x] = upper_body else {
        return None;
    };
    let (upper_x_condition, upper_right_body, upper_left_body) = one_if(upper_x)?;
    if !comparison_with_zero(
        upper_x_condition,
        BinaryOperator::GreaterEqual,
        &x.name,
    ) {
        return None;
    }
    let [upper_right] = upper_right_body else {
        return None;
    };
    let (upper_right_condition, octant_1, octant_2) = one_if(upper_right)?;
    if !comparison(
        upper_right_condition,
        BinaryOperator::GreaterEqual,
        Argument::Plain(&x.name),
        Argument::Plain(&y.name),
    ) {
        return None;
    }
    let callee = direct_leaf_callee(octant_1, result)?;
    if !leaf_matches(
        octant_1,
        result,
        callee,
        [Argument::Plain(&y.name), Argument::Plain(&x.name)],
        ResultAdjustment::Direct,
    ) || !leaf_matches(
        octant_2,
        result,
        callee,
        [Argument::Plain(&x.name), Argument::Plain(&y.name)],
        ResultAdjustment::SubtractFrom(0x4000),
    ) {
        return None;
    }

    let [upper_left] = upper_left_body else {
        return None;
    };
    let (upper_left_condition, octant_3, octant_4) = one_if(upper_left)?;
    if !comparison(
        upper_left_condition,
        BinaryOperator::Less,
        Argument::Negated(&x.name),
        Argument::Plain(&y.name),
    ) || !leaf_matches(
        octant_3,
        result,
        callee,
        [Argument::Negated(&x.name), Argument::Plain(&y.name)],
        ResultAdjustment::Add(0x4000),
    ) || !leaf_matches(
        octant_4,
        result,
        callee,
        [Argument::Plain(&y.name), Argument::Negated(&x.name)],
        ResultAdjustment::SubtractFrom(0x8000),
    ) {
        return None;
    }

    let [negate_y, lower_x] = lower_body else {
        return None;
    };
    if !matches!(
        negate_y,
        Statement::Assign { name, value }
            if name == &y.name && negated_variable(value, &y.name)
    ) {
        return None;
    }
    let (lower_x_condition, lower_left_body, lower_right_body) = one_if(lower_x)?;
    if !comparison_with_zero(lower_x_condition, BinaryOperator::Less, &x.name) {
        return None;
    }
    let [lower_left] = lower_left_body else {
        return None;
    };
    let (lower_left_condition, octant_5, octant_6) = one_if(lower_left)?;
    if !comparison(
        lower_left_condition,
        BinaryOperator::GreaterEqual,
        Argument::Negated(&x.name),
        Argument::Plain(&y.name),
    ) || !leaf_matches(
        octant_5,
        result,
        callee,
        [Argument::Plain(&y.name), Argument::Negated(&x.name)],
        ResultAdjustment::Add(0x8000),
    ) || !leaf_matches(
        octant_6,
        result,
        callee,
        [Argument::Negated(&x.name), Argument::Plain(&y.name)],
        ResultAdjustment::SubtractFrom(0xc000),
    ) {
        return None;
    }

    let [lower_right] = lower_right_body else {
        return None;
    };
    let (lower_right_condition, octant_7, octant_8) = one_if(lower_right)?;
    if !comparison(
        lower_right_condition,
        BinaryOperator::Less,
        Argument::Plain(&x.name),
        Argument::Plain(&y.name),
    ) || !leaf_matches(
        octant_7,
        result,
        callee,
        [Argument::Plain(&x.name), Argument::Plain(&y.name)],
        ResultAdjustment::Add(0xc000),
    ) || !leaf_matches(
        octant_8,
        result,
        callee,
        [Argument::Plain(&y.name), Argument::Plain(&x.name)],
        ResultAdjustment::Negate,
    ) {
        return None;
    }

    let summary = summaries.guarded_float_table_index(callee)?;
    Some(FloatOctantTableDispatch {
        table: summary.table.clone(),
        zero: summary.zero,
        scale: summary.scale,
        bias: summary.bias,
    })
}

impl Generator {
    fn emit_octant_table_load(&mut self, table: &str) {
        self.output
            .data_section_displacements
            .push(DataSectionDisplacement {
                instruction_index: self.output.instructions.len(),
                symbol: table.to_owned(),
            });
        self.output.instructions.push(Instruction::LoadHalfwordZero {
            d: 0,
            a: 3,
            offset: 0,
        });
    }

    fn emit_inlined_guarded_float_table_index(
        &mut self,
        numerator: u8,
        denominator: u8,
        table: &str,
        scale: usize,
        bias: usize,
    ) {
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered {
                a: 0,
                b: denominator,
            });
        let conversion = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, conversion);
        self.emit_octant_table_load(table);
        let joined = self.fresh_label();
        self.emit_branch_to(joined);
        self.bind_label(conversion);
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle {
                d: 0,
                a: numerator,
                b: denominator,
            });
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(scale));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            });
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(bias));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 2,
                a: 0,
                offset: 0,
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
                offset: 32,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.emit_octant_table_load(table);
        self.bind_label(joined);
    }

    fn emit_octant_result(&mut self, adjustment: ResultAdjustment) {
        match adjustment {
            ResultAdjustment::Direct => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 3,
                        s: 0,
                        clear: 16,
                    });
            }
            ResultAdjustment::Add(0x4000) => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 3,
                        s: 0,
                        clear: 16,
                    });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0x4000,
                });
            }
            ResultAdjustment::SubtractFrom(0x4000) => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 0,
                        s: 0,
                        clear: 16,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromImmediate {
                        d: 3,
                        a: 0,
                        immediate: 0x4000,
                    });
            }
            ResultAdjustment::Add(value @ (0x8000 | 0xc000)) => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 3,
                        s: 0,
                        clear: 16,
                    });
                self.output
                    .instructions
                    .push(Instruction::AddImmediateShifted {
                        d: 3,
                        a: 3,
                        immediate: 1,
                    });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: value as i16,
                });
            }
            ResultAdjustment::SubtractFrom(value @ (0x8000 | 0xc000)) => {
                self.output
                    .instructions
                    .push(Instruction::AddImmediateShifted {
                        d: 3,
                        a: 0,
                        immediate: 1,
                    });
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 4,
                        s: 0,
                        clear: 16,
                    });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 0,
                    a: 3,
                    immediate: value as i16,
                });
                self.output
                    .instructions
                    .push(Instruction::SubtractFrom {
                        d: 3,
                        a: 4,
                        b: 0,
                    });
            }
            ResultAdjustment::Negate => {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 0,
                        s: 0,
                        clear: 16,
                    });
                self.output
                    .instructions
                    .push(Instruction::Negate { d: 3, a: 0 });
            }
            _ => unreachable!("classifier admits only the measured octant constants"),
        }
    }

    pub(crate) fn try_float_octant_table_dispatch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function, &self.inline_summaries) else {
            return Ok(false);
        };
        if !self.behavior.legacy_float_cast_schedule
            || self.float_register_of(&function.parameters[0].name)? != 1
            || self.float_register_of(&function.parameters[1].name)? != 2
            || self
                .global_array_sizes
                .get(&shape.table)
                .is_none_or(|size| *size <= 8)
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        // Source traversal sees the callee's guard zero, additive bias, and
        // multiplicative scale in that order. Reuse those pool identities at
        // each expanded call even though the emitted kernel loads scale first.
        let zero = self
            .output
            .intern_constant((shape.zero as f32).to_bits() as u64, 4);
        let bias = self
            .output
            .intern_constant((shape.bias as f32).to_bits() as u64, 4);
        let scale = self
            .output
            .intern_constant((shape.scale as f32).to_bits() as u64, 4);

        self.frame_size = 40;
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![shape.table.clone()];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            });
        self.emit_address_high(3, "...data.0");
        self.emit_address_low(3, "...data.0");
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(zero));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            });

        let lower_half = self.fresh_label();
        let upper_left = self.fresh_label();
        let upper_right_second = self.fresh_label();
        let epilogue = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, lower_half);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, upper_left);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 2 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, upper_right_second);

        self.emit_inlined_guarded_float_table_index(2, 1, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::Direct);
        self.emit_branch_to(epilogue);

        self.bind_label(upper_right_second);
        self.emit_inlined_guarded_float_table_index(1, 2, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::SubtractFrom(0x4000));
        self.emit_branch_to(epilogue);

        self.bind_label(upper_left);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 2 });
        let upper_left_second = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, upper_left_second);
        self.emit_inlined_guarded_float_table_index(1, 2, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::Add(0x4000));
        self.emit_branch_to(epilogue);

        self.bind_label(upper_left_second);
        self.emit_inlined_guarded_float_table_index(2, 1, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::SubtractFrom(0x8000));
        self.emit_branch_to(epilogue);

        self.bind_label(lower_half);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 2, b: 2 });
        let lower_right = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, lower_right);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 2 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        let lower_left_second = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, lower_left_second);
        self.emit_inlined_guarded_float_table_index(2, 1, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::Add(0x8000));
        self.emit_branch_to(epilogue);

        self.bind_label(lower_left_second);
        self.emit_inlined_guarded_float_table_index(1, 2, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::SubtractFrom(0xc000));
        self.emit_branch_to(epilogue);

        self.bind_label(lower_right);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 2 });
        let lower_right_second = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, lower_right_second);
        self.emit_inlined_guarded_float_table_index(1, 2, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::Add(0xc000));
        self.emit_branch_to(epilogue);

        self.bind_label(lower_right_second);
        self.emit_inlined_guarded_float_table_index(2, 1, &shape.table, scale, bias);
        self.emit_octant_result(ResultAdjustment::Negate);

        self.bind_label(epilogue);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 3,
                s: 3,
                clear: 16,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 40,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
