//! Leaf float friction selection with two absolute-value diamonds.
//!
//! This source shape is a small CFG, but its two `fabs` spellings and nested
//! sign adjustment form one scheduled region. Keeping it here avoids teaching
//! the general statement walker partial phi semantics before it owns arbitrary
//! control flow.

#[allow(unused_imports)]
use super::*;

struct FloatFrictionSelect<'a> {
    pointer: &'a str,
    value: &'a str,
    input_offset: i16,
    output_offset: i16,
    inclusive: bool,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn member<'a>(expression: &'a Expression, pointer: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (variable(base)? == pointer)
        .then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn negates(expression: &Expression, operand: &Expression) -> bool {
    matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, operand: inner }
        if same_operand(inner, operand))
}

fn absolute_select<'a>(expression: &'a Expression) -> Option<&'a Expression> {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    (is_zero_literal(right) && negates(when_true, left) && same_operand(when_false, left))
        .then_some(left)
}

fn recognize(function: &Function) -> Option<FloatFrictionSelect<'_>> {
    if function.return_type != Type::Void
        || function.parameters.len() != 2
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [pointer, value] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(pointer.parameter_type, Type::StructPointer { .. })
        || value.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }, Statement::Store {
        target,
        value: stored,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let (output_offset, Type::Float) = member(target, &pointer.name)? else {
        return None;
    };
    if variable(stored) != Some(value.name.as_str()) {
        return None;
    }
    let Expression::Binary {
        operator,
        left: absolute_value,
        right: absolute_member,
    } = condition
    else {
        return None;
    };
    let inclusive = match operator {
        BinaryOperator::Greater => false,
        BinaryOperator::GreaterEqual => true,
        _ => return None,
    };
    let absolute_value = absolute_select(absolute_value)?;
    let absolute_member = absolute_select(absolute_member)?;
    if variable(absolute_value) != Some(value.name.as_str()) {
        return None;
    }
    let (input_offset, Type::Float) = member(absolute_member, &pointer.name)? else {
        return None;
    };
    let [Statement::Assign {
        name: then_name,
        value: then_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if then_name != &value.name || !negates(then_value, absolute_member) {
        return None;
    }
    let [Statement::If {
        condition: inner_condition,
        then_body: inner_then,
        else_body: inner_else,
    }] = else_body.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Greater,
        left: inner_member,
        right: inner_zero,
    } = inner_condition
    else {
        return None;
    };
    if !inner_else.is_empty()
        || !is_zero_literal(inner_zero)
        || !same_operand(inner_member, absolute_member)
    {
        return None;
    }
    let [Statement::Assign {
        name: inner_name,
        value: inner_value,
    }] = inner_then.as_slice()
    else {
        return None;
    };
    if inner_name != &value.name || !negates(inner_value, absolute_value) {
        return None;
    }
    Some(FloatFrictionSelect {
        pointer: &pointer.name,
        value: &value.name,
        input_offset,
        output_offset,
        inclusive,
    })
}

impl Generator {
    pub(crate) fn try_float_friction_select(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.name == "ftCommon_ApplyFrictionAir" {
            eprintln!("{function:#?}");
        }
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        let Some(pointer) = self.lookup_general(shape.pointer) else {
            return Ok(false);
        };
        let value = self.float_register_of(shape.value)?;
        let loaded = 3;
        let absolute_loaded = 2;
        let absolute_value = FLOAT_SCRATCH;
        // Four float-control diamonds participate in MWCC's anonymous-label
        // walk: the two absolute-value selects, the outer clamp, and the nested
        // sign adjustment. `has_float_branch` accounts for one (+3); retain the
        // other three explicitly so the pooled zero receives the same @N name.
        self.output.has_float_branch = true;
        self.output.anonymous_label_bump += 9;

        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: loaded,
            a: pointer,
            offset: shape.input_offset,
        });
        self.load_float_constant(FLOAT_SCRATCH, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: loaded,
                b: FLOAT_SCRATCH,
            });
        let loaded_nonnegative = self.fresh_label();
        let loaded_absolute = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, loaded_nonnegative); // bge
        self.output.instructions.push(Instruction::FloatNegate {
            d: absolute_loaded,
            b: loaded,
        });
        self.emit_branch_to(loaded_absolute);
        self.bind_label(loaded_nonnegative);
        self.output.instructions.push(Instruction::FloatMove {
            d: absolute_loaded,
            b: loaded,
        });
        self.bind_label(loaded_absolute);

        self.load_float_constant(FLOAT_SCRATCH, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: value,
                b: FLOAT_SCRATCH,
            });
        let value_nonnegative = self.fresh_label();
        let value_absolute = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, value_nonnegative); // bge
        self.output.instructions.push(Instruction::FloatNegate {
            d: absolute_value,
            b: value,
        });
        self.emit_branch_to(value_absolute);
        self.bind_label(value_nonnegative);
        self.output.instructions.push(Instruction::FloatMove {
            d: absolute_value,
            b: value,
        });
        self.bind_label(value_absolute);

        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: absolute_value,
                b: absolute_loaded,
            });
        let adjust_sign = self.fresh_label();
        let done = self.fresh_label();
        if shape.inclusive {
            // IEEE `>=` must stay false for unordered values. MWCC folds gt|eq
            // into CR0.eq, then takes the sign-adjustment arm when that bit is
            // clear. A direct `blt` would incorrectly accept NaN here.
            self.output
                .instructions
                .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
            self.emit_branch_conditional_to(4, 2, adjust_sign); // bne
        } else {
            self.emit_branch_conditional_to(4, 1, adjust_sign); // ble
        }
        self.output.instructions.push(Instruction::FloatNegate {
            d: value,
            b: loaded,
        });
        self.emit_branch_to(done);
        self.bind_label(adjust_sign);
        self.load_float_constant(FLOAT_SCRATCH, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: loaded,
                b: FLOAT_SCRATCH,
            });
        self.emit_branch_conditional_to(4, 1, done); // ble
        self.output.instructions.push(Instruction::FloatNegate {
            d: value,
            b: value,
        });
        self.bind_label(done);
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: value,
            a: pointer,
            offset: shape.output_offset,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
