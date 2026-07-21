//! ASCII case-folding string hash loops.
//!
//! The BFBB utility family computes `fold(byte) + hash * 131`, where
//! `fold(byte)` clears ASCII bit 5 only for lower-case characters. Recognize
//! that semantic expression rather than its function or variable names so the
//! lowering remains useful to generated and independently written code.

#[allow(unused_imports)]
use super::*;

enum AsciiHashLoop<'a> {
    NullTerminated {
        pointer: &'a str,
    },
    Bounded {
        pointer: &'a str,
        bound: &'a str,
    },
    PrefixSeeded {
        /// The measured source leaves its local accumulator uninitialized. MWCC
        /// coalesces that local with this otherwise-unused incoming parameter,
        /// making the prefix the observable seed despite the source UB.
        seed: &'a str,
        pointer: &'a str,
    },
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn literal(expression: &Expression, expected: i64) -> bool {
    matches!(expression, Expression::IntegerLiteral(value) if *value == expected)
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

fn is_signed_shift_by_one(expression: &Expression, byte: &str) -> bool {
    let Some((left, right)) = binary(expression, BinaryOperator::ShiftRight) else {
        return false;
    };
    let Expression::Cast {
        target_type: Type::Int,
        operand,
    } = left
    else {
        return false;
    };
    variable(operand) == Some(byte) && literal(right, 1)
}

fn is_ascii_fold(expression: &Expression, byte: &str) -> bool {
    let Some((difference, byte_mask)) = binary(expression, BinaryOperator::BitAnd) else {
        return false;
    };
    if !literal(byte_mask, 0xff) {
        return false;
    }
    let Some((original, case_bit)) = binary(difference, BinaryOperator::Subtract) else {
        return false;
    };
    if variable(original) != Some(byte) {
        return false;
    }
    let Some((case_test, bit_5)) = binary(case_bit, BinaryOperator::BitAnd) else {
        return false;
    };
    if !literal(bit_5, 0x20) {
        return false;
    }
    let Some((same_byte, shifted)) = binary(case_test, BinaryOperator::BitAnd) else {
        return false;
    };
    variable(same_byte) == Some(byte) && is_signed_shift_by_one(shifted, byte)
}

fn is_hash_update(expression: &Expression, byte: &str, accumulator: &str) -> bool {
    let Some((fold, scaled_hash)) = binary(expression, BinaryOperator::Add) else {
        return false;
    };
    let Some((hash, multiplier)) = binary(scaled_hash, BinaryOperator::Multiply) else {
        return false;
    };
    is_ascii_fold(fold, byte) && variable(hash) == Some(accumulator) && literal(multiplier, 131)
}

fn is_pointer_increment(expression: &Expression, pointer: &str) -> bool {
    let Some((base, amount)) = binary(expression, BinaryOperator::Add) else {
        return false;
    };
    variable(base) == Some(pointer) && literal(amount, 1)
}

fn is_plain_local(local: &LocalDeclaration, declared_type: Type) -> bool {
    local.declared_type == declared_type
        && !local.is_const
        && !local.is_static
        && !local.is_volatile
        && local.array_length.is_none()
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
        && local.row_bytes.is_none()
}

fn recognize_null_terminated(function: &Function) -> Option<AsciiHashLoop<'_>> {
    if function.return_type != Type::UnsignedInt
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [pointer] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        pointer.parameter_type,
        Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
    ) {
        return None;
    }
    let [accumulator, byte] = function.locals.as_slice() else {
        return None;
    };
    if !is_plain_local(accumulator, Type::UnsignedInt)
        || !matches!(accumulator.initializer.as_ref(), Some(value) if literal(value, 0))
        || !is_plain_local(byte, Type::UnsignedInt)
        || byte.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value) == Some(accumulator.name.as_str()))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(Expression::Comma { left, right }),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Assign { target, value } = left.as_ref() else {
        return None;
    };
    let Expression::Dereference {
        pointer: loaded_from,
    } = value.as_ref()
    else {
        return None;
    };
    let Some((tested, zero)) = binary(right, BinaryOperator::NotEqual) else {
        return None;
    };
    if variable(target) != Some(byte.name.as_str())
        || variable(loaded_from) != Some(pointer.name.as_str())
        || variable(tested) != Some(byte.name.as_str())
        || !literal(zero, 0)
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_hash,
        value: hash_value,
    }, Statement::Assign {
        name: assigned_pointer,
        value: pointer_value,
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_hash != &accumulator.name
        || !is_hash_update(hash_value, &byte.name, &accumulator.name)
        || assigned_pointer != &pointer.name
        || !is_pointer_increment(pointer_value, &pointer.name)
    {
        return None;
    }
    Some(AsciiHashLoop::NullTerminated {
        pointer: &pointer.name,
    })
}

fn recognize_bounded(function: &Function) -> Option<AsciiHashLoop<'_>> {
    if function.return_type != Type::UnsignedInt
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [pointer, bound] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        pointer.parameter_type,
        Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
    ) || bound.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [accumulator, index, byte] = function.locals.as_slice() else {
        return None;
    };
    if !is_plain_local(accumulator, Type::UnsignedInt)
        || !matches!(accumulator.initializer.as_ref(), Some(value) if literal(value, 0))
        || !is_plain_local(index, Type::UnsignedInt)
        || !matches!(index.initializer.as_ref(), Some(value) if literal(value, 0))
        || !is_plain_local(byte, Type::UnsignedInt)
        || byte.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value) == Some(accumulator.name.as_str()))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition:
            Some(Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            }),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Some((tested_index, tested_bound)) = binary(left, BinaryOperator::Less) else {
        return None;
    };
    let Expression::Comma {
        left: assignment,
        right: byte_test,
    } = right.as_ref()
    else {
        return None;
    };
    let Expression::Assign { target, value } = assignment.as_ref() else {
        return None;
    };
    let Expression::Dereference {
        pointer: loaded_from,
    } = value.as_ref()
    else {
        return None;
    };
    let Some((tested_byte, zero)) = binary(byte_test, BinaryOperator::NotEqual) else {
        return None;
    };
    if variable(tested_index) != Some(index.name.as_str())
        || variable(tested_bound) != Some(bound.name.as_str())
        || variable(target) != Some(byte.name.as_str())
        || variable(loaded_from) != Some(pointer.name.as_str())
        || variable(tested_byte) != Some(byte.name.as_str())
        || !literal(zero, 0)
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_index,
        value: index_value,
    }, Statement::Assign {
        name: assigned_pointer,
        value: pointer_value,
    }, Statement::Assign {
        name: assigned_hash,
        value: hash_value,
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_index != &index.name
        || !is_pointer_increment(index_value, &index.name)
        || assigned_pointer != &pointer.name
        || !is_pointer_increment(pointer_value, &pointer.name)
        || assigned_hash != &accumulator.name
        || !is_hash_update(hash_value, &byte.name, &accumulator.name)
    {
        return None;
    }
    Some(AsciiHashLoop::Bounded {
        pointer: &pointer.name,
        bound: &bound.name,
    })
}

fn recognize_prefix_seeded(function: &Function) -> Option<AsciiHashLoop<'_>> {
    if function.return_type != Type::UnsignedInt
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [seed, pointer] = function.parameters.as_slice() else {
        return None;
    };
    if seed.parameter_type != Type::UnsignedInt
        || !matches!(
            pointer.parameter_type,
            Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
        )
    {
        return None;
    }
    let [accumulator, byte] = function.locals.as_slice() else {
        return None;
    };
    if !is_plain_local(accumulator, Type::UnsignedInt)
        || accumulator.initializer.is_some()
        || !is_plain_local(byte, Type::UnsignedInt)
        || byte.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value) == Some(accumulator.name.as_str()))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(Expression::Comma { left, right }),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Assign { target, value } = left.as_ref() else {
        return None;
    };
    let Expression::Dereference {
        pointer: loaded_from,
    } = value.as_ref()
    else {
        return None;
    };
    let Some((tested, zero)) = binary(right, BinaryOperator::NotEqual) else {
        return None;
    };
    if variable(target) != Some(byte.name.as_str())
        || variable(loaded_from) != Some(pointer.name.as_str())
        || variable(tested) != Some(byte.name.as_str())
        || !literal(zero, 0)
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_pointer,
        value: pointer_value,
    }, Statement::Assign {
        name: assigned_hash,
        value: hash_value,
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_pointer != &pointer.name
        || !is_pointer_increment(pointer_value, &pointer.name)
        || assigned_hash != &accumulator.name
        || !is_hash_update(hash_value, &byte.name, &accumulator.name)
    {
        return None;
    }
    Some(AsciiHashLoop::PrefixSeeded {
        seed: &seed.name,
        pointer: &pointer.name,
    })
}

fn recognize(function: &Function) -> Option<AsciiHashLoop<'_>> {
    recognize_null_terminated(function)
        .or_else(|| recognize_bounded(function))
        .or_else(|| recognize_prefix_seeded(function))
}

mod emit;
#[cfg(test)]
mod tests;
