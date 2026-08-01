//! Native 64-bit bit-mask updates through pointer lvalues.
//!
//! Scalar load/store selection deliberately rejects long long because it has
//! no single instruction. This owner keeps the pair semantics explicit: load
//! both big-endian words, mask each independently, and store both words back.

use super::*;

enum WideMaskSource<'a> {
    Wide(&'a Expression),
    Word(&'a Expression),
}

fn wide_pointer_bitand<'a>(
    target: &'a Expression,
    mut value: &'a Expression,
) -> Option<(&'a Expression, WideMaskSource<'a>, u64)> {
    let Expression::Dereference { pointer } = target else {
        return None;
    };
    if !matches!(
        pointer.as_ref(),
        Expression::Cast {
            target_type: Type::Pointer(Pointee::LongLong | Pointee::UnsignedLongLong),
            ..
        }
    ) {
        return None;
    }
    if let Expression::IndexedUpdateValue { value: inner } = value {
        value = inner;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = value
    else {
        return None;
    };
    let source = if structurally_equal(target, left) {
        WideMaskSource::Wide(pointer)
    } else if let Expression::Dereference {
        pointer: source_pointer,
    } = left.as_ref()
    {
        let (
            Expression::Cast {
                operand: destination_base,
                ..
            },
            Expression::Cast {
                target_type: Type::Pointer(Pointee::Int | Pointee::UnsignedInt),
                operand: source_base,
            },
        ) = (pointer.as_ref(), source_pointer.as_ref())
        else {
            return None;
        };
        if !structurally_equal(destination_base, source_base) {
            return None;
        }
        WideMaskSource::Word(source_pointer)
    } else {
        return None;
    };
    Some((pointer, source, constant_value(right)? as u64))
}

impl Generator {
    pub(crate) fn try_emit_wide_pointer_mask_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((pointer, source, mask)) = wide_pointer_bitand(target, value) else {
            return Ok(false);
        };
        let base = self.fresh_virtual_general_preferring(3);
        self.evaluate_general(pointer, base)?;

        let high = self.fresh_virtual_general();
        let low = self.fresh_virtual_general();
        match source {
            WideMaskSource::Wide(source_pointer) => {
                debug_assert!(structurally_equal(pointer, source_pointer));
                self.output.instructions.push(Instruction::LoadWord {
                    d: low,
                    a: base,
                    offset: 4,
                });
                self.output.instructions.push(Instruction::LoadWord {
                    d: high,
                    a: base,
                    offset: 0,
                });
            }
            WideMaskSource::Word(source_pointer) => {
                let source_base = self.fresh_virtual_general();
                self.evaluate_general(source_pointer, source_base)?;
                self.output.instructions.push(Instruction::LoadWord {
                    d: low,
                    a: source_base,
                    offset: 0,
                });
                self.load_integer_constant(high, 0);
            }
        }

        self.load_integer_constant(GENERAL_SCRATCH, mask as u32 as i32 as i64);
        self.output.instructions.push(Instruction::And {
            a: low,
            s: low,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: low,
            a: base,
            offset: 4,
        });

        self.load_integer_constant(GENERAL_SCRATCH, (mask >> 32) as u32 as i32 as i64);
        self.output.instructions.push(Instruction::And {
            a: high,
            s: high,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: high,
            a: base,
            offset: 0,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_big_endian_wide_pointer_mask_update() {
        let pointer = Expression::Cast {
            target_type: Type::Pointer(Pointee::UnsignedLongLong),
            operand: Box::new(Expression::Variable("value".into())),
        };
        let target = Expression::Dereference {
            pointer: Box::new(pointer),
        };
        let value = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(target.clone()),
            right: Box::new(Expression::IntegerLiteral(0xffff_ffff)),
        };

        let (recognized_pointer, source, mask) = wide_pointer_bitand(&target, &value).unwrap();
        assert_eq!(mask, 0xffff_ffff);
        assert!(matches!(source, WideMaskSource::Wide(_)));
        assert!(matches!(
            recognized_pointer,
            Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == "value")
        ));
    }
}
