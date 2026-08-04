//! Read/modify/write transactions on file-scope arrays.
//!
//! A global-array update must form one element address and use it for both the
//! load and store.  Routing the expression through the ordinary store owner
//! loses that identity and treats the computed update value as an unrelated
//! non-register source.

#[allow(unused_imports)]
use super::*;

struct GlobalArrayUpdate<'a> {
    name: &'a str,
    index: &'a Expression,
    operator: BinaryOperator,
    right: &'a Expression,
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<GlobalArrayUpdate<'a>> {
    let Expression::Index { base, index } = target else {
        return None;
    };
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator,
        left,
        right,
    } = value
    else {
        return None;
    };
    if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
        || !structurally_equal(target, left)
    {
        return None;
    }
    Some(GlobalArrayUpdate {
        name,
        index,
        operator: *operator,
        right,
    })
}

impl Generator {
    pub(super) fn try_emit_global_array_indexed_rmw(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some(update) = classify(target, value) else {
            return Ok(false);
        };
        let Some(&total_size) = self.global_array_sizes.get(update.name) else {
            return Ok(false);
        };
        let Some(element_type) = self.globals.get(update.name).copied() else {
            return Ok(false);
        };
        let Some(element) = pointee_of_type(element_type) else {
            return Ok(false);
        };
        if !matches!(
            element,
            Pointee::Char
                | Pointee::UnsignedChar
                | Pointee::Short
                | Pointee::UnsignedShort
                | Pointee::Int
                | Pointee::UnsignedInt
        ) || expression_has_call(update.index)
            || expression_has_call(update.right)
        {
            return Ok(false);
        }
        let Some(immediate) = constant_value(update.right)
            .and_then(|value| {
                if update.operator == BinaryOperator::Subtract {
                    value.checked_neg()
                } else {
                    Some(value)
                }
            })
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let index = self.materialize_index_operand(update.index)?;
        let scaled = self.fresh_virtual_general_preferring(index);
        let element_size = element.size();
        if element_size.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index,
                    shift: element_size.trailing_zeros() as u8,
                });
        } else {
            let scale = i16::try_from(element_size).map_err(|_| {
                Diagnostic::error("global-array update element size is out of range")
            })?;
            self.output.instructions.push(Instruction::MultiplyImmediate {
                d: scaled,
                a: index,
                immediate: scale,
            });
        }
        let base = self.fresh_virtual_general();
        self.emit_global_array_base(update.name, total_size, base)?;
        let loaded = self.fresh_virtual_general();
        self.output
            .instructions
            .push(indexed_load(element, loaded, base, scaled)?);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: loaded,
            immediate,
        });
        self.output.instructions.push(indexed_store(
            element,
            GENERAL_SCRATCH,
            base,
            scaled,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot() -> Expression {
        Expression::Index {
            base: Box::new(Expression::Variable("values".into())),
            index: Box::new(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("cursor".into())),
                right: Box::new(Expression::IntegerLiteral(31)),
            }),
        }
    }

    #[test]
    fn recognizes_an_increment_of_the_same_global_array_element() {
        let target = slot();
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(slot()),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let update = classify(&target, &value).expect("same-slot increment should classify");
        assert_eq!(update.name, "values");
        assert_eq!(update.operator, BinaryOperator::Add);
    }

    #[test]
    fn rejects_an_update_loaded_from_a_different_slot() {
        let target = slot();
        let value = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Index {
                base: Box::new(Expression::Variable("other".into())),
                index: Box::new(Expression::Variable("cursor".into())),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        assert!(classify(&target, &value).is_none());
    }
}
