//! Bounded member cursors with a conditional high-water update.
//!
//! This is a small but reusable nested leaf diamond: reject an out-of-range
//! cursor with an error value, otherwise store the cursor and raise a second
//! member only when the new value exceeds it. Metrowerks keeps the returned
//! error local in the first free argument register across both arms.

#[allow(unused_imports)]
use super::*;

struct BoundedMemberCursor<'a> {
    base: &'a str,
    cursor: &'a str,
    bound: u16,
    success: i16,
    failure: i16,
    position_offset: i16,
    high_water_offset: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn member_of<'a>(expression: &'a Expression, base: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base: member_base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(member_base, base).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn member_store(statement: &Statement, base: &str, value: &str) -> Option<(i16, Type)> {
    let Statement::Store {
        target,
        value: stored,
    } = statement
    else {
        return None;
    };
    if !variable(stored, value) {
        return None;
    }
    member_of(target, base)
}

fn classify(function: &Function) -> Option<BoundedMemberCursor<'_>> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt) || !function.guards.is_empty()
    {
        return None;
    }
    let [base, cursor] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::StructPointer { .. })
        || cursor.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(result.declared_type, Type::Int | Type::UnsignedInt)
        || result.array_length.is_some()
        || result.is_static
        || result.is_volatile
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, &result.name))
    {
        return None;
    }
    let success = i16::try_from(constant_value(result.initializer.as_ref()?)?).ok()?;

    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::Greater,
            left,
            right,
        } if variable(left, &cursor.name) => u16::try_from(constant_value(right)?).ok()?,
        _ => return None,
    };
    let [Statement::Assign {
        name: assigned,
        value: failure,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if assigned != &result.name {
        return None;
    }
    let failure = i16::try_from(constant_value(failure)?).ok()?;

    let [position_store, Statement::If {
        condition: high_water_condition,
        then_body: high_water_body,
        else_body: high_water_else,
    }] = else_body.as_slice()
    else {
        return None;
    };
    if !high_water_else.is_empty() {
        return None;
    }
    let (position_offset, position_type) = member_store(position_store, &base.name, &cursor.name)?;
    if position_type != Type::UnsignedInt {
        return None;
    }
    let high_water_offset = match high_water_condition {
        Expression::Binary {
            operator: BinaryOperator::Greater,
            left,
            right,
        } if variable(left, &cursor.name) => {
            let (offset, member_type) = member_of(right, &base.name)?;
            (member_type == Type::UnsignedInt).then_some(offset)?
        }
        _ => return None,
    };
    let [high_water_store] = high_water_body.as_slice() else {
        return None;
    };
    let (stored_high_water_offset, high_water_type) =
        member_store(high_water_store, &base.name, &cursor.name)?;
    if high_water_type != Type::UnsignedInt || stored_high_water_offset != high_water_offset {
        return None;
    }

    Some(BoundedMemberCursor {
        base: &base.name,
        cursor: &cursor.name,
        bound,
        success,
        failure,
        position_offset,
        high_water_offset,
    })
}

impl Generator {
    /// Lower the bounded cursor/high-water nested diamond as one control-flow
    /// region, preserving mwcc's comparison and error-local schedule.
    pub(crate) fn try_bounded_member_cursor(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.base)?;
        let cursor = self.general_register_of(shape.cursor)?;
        if base != 3 || cursor != 4 || !self.frame_slots.is_empty() {
            return Ok(false);
        }

        let result_home = 5;
        let success_path = self.fresh_label();
        let join = self.fresh_label();
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: cursor,
                immediate: shape.bound,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(result_home, shape.success));
        self.emit_branch_conditional_to(4, 1, success_path); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(result_home, shape.failure));
        self.emit_branch_to(join);

        self.bind_label(success_path);
        self.output.instructions.push(Instruction::StoreWord {
            s: cursor,
            a: base,
            offset: shape.position_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: base,
            offset: shape.high_water_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: cursor,
                b: GENERAL_SCRATCH,
            });
        self.emit_branch_conditional_to(4, 1, join); // ble
        self.output.instructions.push(Instruction::StoreWord {
            s: cursor,
            a: base,
            offset: shape.high_water_offset,
        });

        self.bind_label(join);
        self.output.instructions.push(Instruction::move_register(
            Eabi::general_result().number,
            result_home,
        ));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
