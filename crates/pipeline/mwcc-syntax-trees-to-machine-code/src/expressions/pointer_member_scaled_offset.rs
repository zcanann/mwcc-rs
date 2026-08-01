//! Pointer members advanced by a scaled integer member and a constant bias.
//!
//! THP-style stream walkers form addresses as `owner->ptr + state.count * 4 + 8`.
//! MWCC keeps the loaded pointer alive, scales the count through r0 into the
//! first result lane, applies the bias, and only then combines the pointer. That
//! schedule is distinct from both ordinary pointer indexing and generic add-tree
//! reassociation, so it has a deliberately narrow owner here.

use super::*;

fn parts(
    expression: &Expression,
) -> Option<(&Expression, &Expression, u8, i16)> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: sum,
        right: bias,
    } = expression
    else {
        return None;
    };
    let bias = constant_value(bias).and_then(|value| i16::try_from(value).ok())?;
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: pointer,
        right: product,
    } = sum.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        member_type: Type::Pointer(_),
        index_stride: None,
        ..
    } = pointer.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: count,
        right: scale,
    } = product.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        member_type: Type::Int | Type::UnsignedInt,
        index_stride: None,
        ..
    } = count.as_ref()
    else {
        return None;
    };
    let scale = constant_value(scale).and_then(|value| u32::try_from(value).ok())?;
    if scale < 2 || !scale.is_power_of_two() {
        return None;
    }
    let shift = u8::try_from(scale.trailing_zeros()).ok()?;
    Some((pointer, count, shift, bias))
}

impl Generator {
    pub(crate) fn try_emit_pointer_member_scaled_offset(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let Some((pointer, count, shift, bias)) = parts(expression) else {
            return Ok(false);
        };

        // With an uncached absolute global aggregate, MWCC starts the global
        // address before loading the independent pointer member. The low half
        // remains on the later count load. A structured-body cache (the THP
        // case) already owns that address and needs no local materialization.
        let count_global = match count {
            Expression::Member {
                base,
                offset: 0,
                index_stride: None,
                ..
            } => match base.as_ref() {
                Expression::Variable(name)
                    if self.addressable_globals.contains_key(name.as_str())
                        && self.structured_global_base_register(name).is_none()
                        && self.data_section_anchor.is_none()
                        && self.behavior.global_addressing == GlobalAddressing::Absolute
                        && self.behavior.absolute_access_style
                            != mwcc_versions::AbsoluteAccessStyle::MaterializedAddress =>
                {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        };
        let count_base = count_global.as_ref().map(|name| {
            let base = self.fresh_virtual_general();
            self.emit_address_high(base, name);
            base
        });

        // The pointer remains live across the count load. A virtual lets the
        // allocator choose r5 when a global-address base occupies r4, but r4 in
        // the THP body where the global base is retained in r31.
        let pointer_value = self.fresh_virtual_general();
        self.evaluate_general(pointer, pointer_value)?;
        if let (Some(name), Some(base)) = (count_global.as_ref(), count_base) {
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: base,
                offset: 0,
            });
        } else {
            self.evaluate_general(count, GENERAL_SCRATCH)?;
        }

        // r3 is MWCC's preferred transient result lane. Whole-body liveness can
        // still displace it when an incoming value survives this expression.
        let scaled = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: GENERAL_SCRATCH,
                shift,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: scaled,
            immediate: bias,
        });
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: pointer_value,
            b: destination,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(base: &str, offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_pointer_plus_scaled_member_plus_bias() {
        let expression = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(member(
                    "read_buffer",
                    0,
                    Type::Pointer(Pointee::UnsignedChar),
                )),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: Box::new(member("player", 108, Type::UnsignedInt)),
                    right: Box::new(Expression::IntegerLiteral(4)),
                }),
            }),
            right: Box::new(Expression::IntegerLiteral(8)),
        };

        let (_, _, shift, bias) = parts(&expression).expect("recognized shape");
        assert_eq!((shift, bias), (2, 8));
    }
}
