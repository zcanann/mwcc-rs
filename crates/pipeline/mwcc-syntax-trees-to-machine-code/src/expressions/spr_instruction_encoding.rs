//! Runtime construction of PowerPC SPR transfer instructions.
//!
//! `mfspr` and `mtspr` split the ten-bit SPR number across two disjoint
//! instruction fields. The macro-expanded C expression exposes two masked
//! shifts of the same source. Legacy optimized MWCC keeps each mask and shift
//! separate while it constructs the instruction word, rather than applying
//! the ordinary shift/mask fusion used for an isolated expression.

use super::*;

#[derive(Debug, Clone, Copy)]
struct SprInstructionEncoding<'a> {
    source: &'a Expression,
    low_bits: u16,
}

fn collect_or_terms<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = expression
    {
        collect_or_terms(left, terms);
        collect_or_terms(right, terms);
    } else {
        terms.push(expression);
    }
}

fn masked_shift(expression: &Expression) -> Option<(&Expression, u32, u8)> {
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let shift = u8::try_from(constant_value(right)?).ok()?;
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    if let Some(mask) = constant_value(right) {
        Some((left, mask as u32, shift))
    } else {
        Some((right, constant_value(left)? as u32, shift))
    }
}

fn classify(expression: &Expression) -> Option<SprInstructionEncoding<'_>> {
    let mut terms = Vec::new();
    collect_or_terms(expression, &mut terms);

    let mut fixed = 0u32;
    let mut upper = None;
    let mut lower = None;
    for term in terms {
        if let Some(constant) = constant_value(term) {
            fixed |= constant as u32;
            continue;
        }
        match masked_shift(term)? {
            (source, 0x0fe0, 6) if upper.is_none() => upper = Some(source),
            (source, 0x001f, 16) if lower.is_none() => lower = Some(source),
            _ => return None,
        }
    }
    let (Some(upper), Some(lower)) = (upper, lower) else {
        return None;
    };
    if !structurally_equal(upper, lower) || fixed & 0xffff_0000 != 0x7c80_0000 {
        return None;
    }
    Some(SprInstructionEncoding {
        source: upper,
        low_bits: fixed as u16,
    })
}

impl Generator {
    pub(crate) fn try_emit_spr_instruction_encoding(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(encoding) = classify(expression) else {
            return Ok(false);
        };
        let source = match self.general_register_of_leaf(encoding.source) {
            Ok(source) if source != destination => source,
            _ => return Ok(false),
        };
        // The surrounding helper commonly forwards r3 and r5 to the eventual
        // cache-flush call. Those values have no explicit machine use until
        // call emission, so keep this short-lived field value out of their ABI
        // homes and prefer MWCC's first non-argument scratch register.
        let upper = self.fresh_virtual_general_avoiding(vec![3, 4, 5]);
        self.prefer_virtual_general(upper, 6);
        let base = self.fresh_virtual_general_preferring(4);

        self.output.instructions.push(Instruction::RotateAndMask {
            a: destination,
            s: source,
            shift: 0,
            begin: 20,
            end: 26,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: upper,
                s: destination,
                shift: 6,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: destination,
                s: source,
                clear: 27,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: base,
                s: upper,
                immediate: 0x7c80,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: destination,
                s: destination,
                shift: 16,
            });
        self.output.instructions.push(Instruction::Or {
            a: destination,
            s: base,
            b: destination,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: destination,
            s: destination,
            immediate: encoding.low_bits,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn field(source: &str, mask: i64, shift: i64) -> Expression {
        binary(
            BinaryOperator::ShiftLeft,
            binary(
                BinaryOperator::BitAnd,
                Expression::Variable(source.into()),
                Expression::IntegerLiteral(mask),
            ),
            Expression::IntegerLiteral(shift),
        )
    }

    fn encoding(mask: i64) -> Expression {
        binary(
            BinaryOperator::BitOr,
            binary(
                BinaryOperator::BitOr,
                binary(
                    BinaryOperator::BitOr,
                    Expression::IntegerLiteral(0x7c80_0000),
                    field("spr", mask, 6),
                ),
                field("spr", 0x1f, 16),
            ),
            Expression::IntegerLiteral(0x2a6),
        )
    }

    #[test]
    fn recognizes_the_two_disjoint_spr_number_fields() {
        let expression = encoding(0xfe0);
        let encoding = classify(&expression).expect("SPR instruction encoding");
        assert!(matches!(encoding.source, Expression::Variable(name) if name == "spr"));
        assert_eq!(encoding.low_bits, 0x2a6);
    }

    #[test]
    fn rejects_a_different_masked_shift_packet() {
        assert!(classify(&encoding(0xff0)).is_none());
    }
}
