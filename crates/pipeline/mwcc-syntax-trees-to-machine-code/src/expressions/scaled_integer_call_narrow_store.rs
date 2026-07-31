//! Narrow stores of a scaled integer call result.
//!
//! Legacy O0 MWCC treats `(char)(scale * (integer_call() / divisor))` as one
//! scheduled transaction.  The call result remains in a GPR while the signed
//! integer-to-float bias image is built, then the scaled float is converted
//! back through the narrow-store conversion image.

use super::*;

struct ScaledIntegerCallNarrowStore<'a> {
    base: &'a Expression,
    offset: u32,
    callee: &'a str,
    scale: f64,
    divisor: f64,
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<ScaledIntegerCallNarrowStore<'a>> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Char | Type::UnsignedChar,
        index_stride: None,
    } = target
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Char | Type::UnsignedChar,
        operand,
    } = value
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = operand.as_ref()
    else {
        return None;
    };
    let (scale, quotient) = match (left.as_ref(), right.as_ref()) {
        (Expression::FloatLiteral(scale), quotient)
        | (quotient, Expression::FloatLiteral(scale)) => (*scale, quotient),
        _ => return None,
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: numerator,
        right: divisor,
    } = quotient
    else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = numerator.as_ref()
    else {
        return None;
    };
    let Expression::FloatLiteral(divisor) = divisor.as_ref() else {
        return None;
    };
    arguments
        .is_empty()
        .then_some(ScaledIntegerCallNarrowStore {
            base,
            offset: *offset,
            callee,
            scale,
            divisor: *divisor,
        })
}

impl Generator {
    pub(crate) fn try_emit_scaled_integer_call_narrow_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O0 {
            return Ok(false);
        }
        let Some(store) = classify(target, value) else {
            return Ok(false);
        };
        if self.call_return_types.get(store.callee) != Some(&Type::Int) {
            return Ok(false);
        }

        let base = self.general_register_of_leaf(store.base)?;
        let offset = i16::try_from(store.offset)
            .map_err(|_| Diagnostic::error("scaled narrow-store offset is out of range"))?;

        self.emit_call(store.callee, &[], None, false)?;
        let call_result = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        self.output.instructions.push(Instruction::move_register(
            call_result,
            Eabi::general_result().number,
        ));

        let int_scratch = self.claim_int_to_float_scratch()?;
        let promoted = self.fresh_virtual_float_preferring(Eabi::float_result().number);
        self.load_double_constant(promoted, 0x4330_0000_8000_0000);
        let biased = self.fresh_virtual_general_preferring(4);
        self.output
            .instructions
            .push(Instruction::XorImmediateShifted {
                a: biased,
                s: call_result,
                immediate: 0x8000,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: biased,
            a: 1,
            offset: int_scratch + 4,
        });
        let high_word = self.fresh_virtual_general_preferring(4);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(high_word, 17200));
        self.output.instructions.push(Instruction::StoreWord {
            s: high_word,
            a: 1,
            offset: int_scratch,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: FLOAT_SCRATCH,
            a: 1,
            offset: int_scratch,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: promoted,
                a: FLOAT_SCRATCH,
                b: promoted,
            });

        self.load_float_literal(FLOAT_SCRATCH, store.divisor, false);
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle {
                d: promoted,
                a: promoted,
                b: FLOAT_SCRATCH,
            });
        self.load_float_literal(FLOAT_SCRATCH, store.scale, false);
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: FLOAT_SCRATCH,
                a: FLOAT_SCRATCH,
                c: promoted,
            });

        let float_scratch = self.claim_float_to_int_scratch()?;
        self.output.has_conversion = true;
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero {
                d: FLOAT_SCRATCH,
                b: FLOAT_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: FLOAT_SCRATCH,
                a: 1,
                offset: float_scratch,
            });
        let result = self.fresh_virtual_general_preferring(Eabi::general_result().number);
        self.output.instructions.push(Instruction::LoadWord {
            d: result,
            a: 1,
            offset: float_scratch + 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: result,
                s: result,
                clear: 24,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: result,
            a: base,
            offset,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_commuted_scale_around_an_integer_call_quotient() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 98,
            member_type: Type::UnsignedChar,
            index_stride: None,
        };
        let quotient = Expression::Binary {
            operator: BinaryOperator::Divide,
            left: Box::new(Expression::Call {
                name: "sample".into(),
                arguments: vec![],
            }),
            right: Box::new(Expression::FloatLiteral(65536.0)),
        };
        let value = Expression::Cast {
            target_type: Type::Char,
            operand: Box::new(Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: Box::new(quotient),
                right: Box::new(Expression::FloatLiteral(8.0)),
            }),
        };

        let store = classify(&target, &value).expect("scaled call store");
        assert_eq!(store.callee, "sample");
        assert_eq!(store.scale, 8.0);
        assert_eq!(store.divisor, 65536.0);
        assert_eq!(store.offset, 98);
    }
}
