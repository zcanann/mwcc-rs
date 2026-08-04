//! Preserve paired float-to-unsigned arguments across their hidden helper calls.
//!
//! A variadic report such as `(format, i, (u32)values[i], i + 1,
//! (u32)values[i + 1])` contains two implicit `__cvt_fp2unsigned` calls. MWCC
//! evaluates the right conversion first, retains its result in a callee-saved
//! value, then evaluates the left conversion. Keeping this schedule separate
//! from ordinary argument marshaling makes the hidden call lifetime explicit.

use super::*;

struct PairedIndexedConversion<'a> {
    first_operand: &'a Expression,
    second_operand: &'a Expression,
}

impl Generator {
    pub(crate) fn try_emit_paired_indexed_float_to_unsigned_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        if !self.variadic_callees.contains(name) {
            return Ok(false);
        }
        let Some(pair) = paired_indexed_conversion(arguments) else {
            return Ok(false);
        };
        if !self.is_float_value(pair.first_operand)
            || !self.is_float_value(pair.second_operand)
        {
            return Ok(false);
        }

        let retained_second = self.fresh_virtual_general();
        self.emit_float_to_unsigned_integer(
            pair.second_operand,
            Eabi::general_result().number,
        )?;
        self.output.instructions.push(Instruction::move_register(
            retained_second,
            Eabi::general_result().number,
        ));
        self.emit_float_to_unsigned_integer(
            pair.first_operand,
            Eabi::general_result().number,
        )?;
        self.output.instructions.push(Instruction::move_register(
            Eabi::FIRST_GENERAL_ARGUMENT + 2,
            Eabi::general_result().number,
        ));

        self.evaluate_general(&arguments[1], Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        self.output.instructions.push(Instruction::move_register(
            Eabi::FIRST_GENERAL_ARGUMENT + 4,
            retained_second,
        ));
        self.evaluate_general(&arguments[0], Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.evaluate_general(&arguments[3], Eabi::FIRST_GENERAL_ARGUMENT + 3)?;
        Ok(true)
    }
}

fn paired_indexed_conversion(arguments: &[Expression]) -> Option<PairedIndexedConversion<'_>> {
    let [
        Expression::StringLiteral(_),
        Expression::Variable(index),
        first,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left: next_base,
            right: next_offset,
        },
        second,
    ] = arguments
    else {
        return None;
    };
    if !matches!(next_base.as_ref(), Expression::Variable(name) if name == index)
        || constant_value(next_offset) != Some(1)
    {
        return None;
    }
    let (first_operand, first_base, first_index) = unsigned_indexed_conversion(first)?;
    let (second_operand, second_base, second_index) = unsigned_indexed_conversion(second)?;
    if !structurally_equal(first_base, second_base)
        || constant_value(second_index) != constant_value(first_index)?.checked_add(1)
    {
        return None;
    }
    Some(PairedIndexedConversion {
        first_operand,
        second_operand,
    })
}

fn unsigned_indexed_conversion(
    expression: &Expression,
) -> Option<(&Expression, &Expression, &Expression)> {
    let Expression::Cast {
        target_type: Type::UnsignedInt,
        operand,
    } = expression
    else {
        return None;
    };
    let Expression::Index { base, index } = operand.as_ref() else {
        return None;
    };
    Some((operand, base, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversion(cursor: &str, index: i64) -> Expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(Expression::Index {
                base: Box::new(Expression::Variable(cursor.into())),
                index: Box::new(Expression::IntegerLiteral(index)),
            }),
        }
    }

    #[test]
    fn recognizes_adjacent_indexed_conversion_arguments() {
        let arguments = vec![
            Expression::StringLiteral(Vec::new()),
            Expression::Variable("i".into()),
            conversion("cursor", 18),
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            },
            conversion("cursor", 19),
        ];

        assert!(paired_indexed_conversion(&arguments).is_some());
    }

    #[test]
    fn rejects_conversion_arguments_from_different_cursor_lanes() {
        let arguments = vec![
            Expression::StringLiteral(Vec::new()),
            Expression::Variable("i".into()),
            conversion("first", 18),
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            },
            conversion("second", 19),
        ];

        assert!(paired_indexed_conversion(&arguments).is_none());
    }
}
