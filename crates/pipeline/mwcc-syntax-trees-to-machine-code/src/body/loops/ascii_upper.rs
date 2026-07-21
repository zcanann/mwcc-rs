//! In-place NUL-terminated ASCII uppercase loops.

#[allow(unused_imports)]
use super::*;

struct AsciiUppercase<'a> {
    input: &'a str,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn literal(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn binary<'a>(
    expression: &'a Expression,
    expected: BinaryOperator,
) -> Option<(&'a Expression, &'a Expression)> {
    match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } if *operator == expected => Some((left, right)),
        _ => None,
    }
}

fn byte_pointer(value: Type) -> bool {
    matches!(value, Type::Pointer(Pointee::Char | Pointee::UnsignedChar))
}

fn dereferences(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer: value }
        if variable(value) == Some(pointer))
}

fn ascii_range(expression: &Expression, pointer: &str) -> bool {
    let Some((lower, upper)) = binary(expression, BinaryOperator::LogicalAnd) else {
        return false;
    };
    binary(lower, BinaryOperator::GreaterEqual)
        .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 97))
        && binary(upper, BinaryOperator::LessEqual)
            .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 122))
}

fn uppercase_value(expression: &Expression, pointer: &str) -> bool {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return false;
    };
    ascii_range(condition, pointer)
        && binary(when_true, BinaryOperator::Subtract)
            .is_some_and(|(byte, amount)| dereferences(byte, pointer) && literal(amount, 32))
        && dereferences(when_false, pointer)
}

fn recognize(function: &Function) -> Option<AsciiUppercase<'_>> {
    if !byte_pointer(function.return_type)
        || !function.guards.is_empty()
        || function_makes_call(function)
        || function.asm_body.is_some()
    {
        return None;
    }
    let [input] = function.parameters.as_slice() else {
        return None;
    };
    let [cursor] = function.locals.as_slice() else {
        return None;
    };
    if !byte_pointer(input.parameter_type)
        || !byte_pointer(cursor.declared_type)
        || !matches!(cursor.initializer.as_ref(), Some(value)
            if variable(value) == Some(input.name.as_str()))
        || cursor.is_const
        || cursor.is_static
        || cursor.is_volatile
        || cursor.array_length.is_some()
        || cursor.data_bytes.is_some()
        || !cursor.data_relocations.is_empty()
        || cursor.row_bytes.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value) == Some(input.name.as_str()))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Some((loaded, zero)) = binary(condition, BinaryOperator::NotEqual) else {
        return None;
    };
    let [Statement::Store { target, value }, Statement::Assign {
        name,
        value: increment,
    }] = body.as_slice()
    else {
        return None;
    };
    if !dereferences(loaded, &cursor.name)
        || !literal(zero, 0)
        || !dereferences(target, &cursor.name)
        || !uppercase_value(value, &cursor.name)
        || name != &cursor.name
        || !binary(increment, BinaryOperator::Add).is_some_and(|(base, amount)| {
            variable(base) == Some(cursor.name.as_str()) && literal(amount, 1)
        })
    {
        return None;
    }
    Some(AsciiUppercase { input: &input.name })
}

impl Generator {
    pub(crate) fn try_ascii_uppercase_loop(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = recognize(function) else {
            return Ok(false);
        };
        let Some(input) = self.lookup_general(plan.input) else {
            return Ok(false);
        };
        if input != Eabi::FIRST_GENERAL_ARGUMENT || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        self.output.pre_scheduled = true;
        let cursor = 5;
        let byte = 4;
        self.output
            .instructions
            .push(Instruction::move_register(cursor, input));
        let body = self.fresh_label();
        let range_done = self.fresh_label();
        let store = self.fresh_label();
        let test = self.fresh_label();
        self.emit_branch_to(test);
        self.bind_label(body);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: byte,
                immediate: 97,
            });
        self.load_integer_constant(0, 0);
        self.emit_branch_conditional_to(12, 0, range_done); // blt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: byte,
                immediate: 122,
            });
        self.emit_branch_conditional_to(12, 1, range_done); // bgt
        self.load_integer_constant(0, 1);
        self.bind_label(range_done);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 24,
            });
        self.emit_branch_conditional_to(12, 2, store); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: byte,
            a: byte,
            immediate: -32,
        });
        self.bind_label(store);
        self.output.instructions.push(Instruction::StoreByte {
            s: byte,
            a: cursor,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: cursor,
            a: cursor,
            immediate: 1,
        });
        self.bind_label(test);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: byte,
            a: cursor,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: byte,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, body); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

#[cfg(test)]
mod tests;
