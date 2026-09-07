//! Operand placement for masked pointer subscripts paired with memory loads.
//! The arithmetic emitter owns the final operator; mask selection stays shared
//! with ordinary subscripts. This owner only selects lifetimes and issue order.

use super::*;

struct ResidentMaskedSubscript<'a> {
    pointee: Pointee,
    address: u8,
    index: masked_index::MaskedIndex<'a>,
}

impl Generator {
    /// Restrict the sibling to a single integer load through a resident pointer.
    /// Signed bytes need an additional extension and remain with the general
    /// lowering path, as do global addresses, indexed members, casts, and calls.
    fn is_resident_displacement_load(&self, expression: &Expression) -> bool {
        let (base, integer) = match expression {
            Expression::Index { base, index } if constant_value(index).is_some() => {
                let Some(pointee) = self.pointee_of(base).ok() else {
                    return false;
                };
                if constant_value(index)
                    .and_then(|index| index.checked_mul(i64::from(pointee.size())))
                    .and_then(|offset| i16::try_from(offset).ok())
                    .is_none()
                {
                    return false;
                }
                (base.as_ref(), Some(pointee.element()))
            }
            Expression::Dereference { pointer } => (
                pointer.as_ref(),
                self.pointee_of(pointer).ok().map(|p| p.element()),
            ),
            Expression::Member {
                base,
                member_type,
                offset,
                index_stride: None,
            } if i16::try_from(*offset).is_ok() => (base.as_ref(), Some(*member_type)),
            _ => return false,
        };
        matches!(
            integer,
            Some(
                Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Int
                    | Type::UnsignedInt
            )
        ) && leaf_name(base).is_some_and(|name| {
            self.locations.contains_key(name) && !self.frame_slots.contains_key(name)
        })
    }

    fn resident_masked_subscript<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<ResidentMaskedSubscript<'a>> {
        let Expression::Index { base, index } = expression else {
            return None;
        };
        let Some(location) = leaf_name(base)
            .filter(|name| !self.frame_slots.contains_key(*name))
            .and_then(|name| self.locations.get(name))
        else {
            return None;
        };
        let Some(
            pointee @ (Pointee::UnsignedChar
            | Pointee::Short
            | Pointee::UnsignedShort
            | Pointee::Int
            | Pointee::UnsignedInt),
        ) = location.pointee
        else {
            return None;
        };
        let address = location.register;
        if address == GENERAL_SCRATCH {
            return None;
        }
        let Some(index) = self.masked_index_with_bias(index, pointee) else {
            return None;
        };
        if index.loaded && !self.is_resident_displacement_load(index.source) {
            return None;
        }
        Some(ResidentMaskedSubscript {
            pointee,
            address,
            index,
        })
    }

    fn place_two_masked_subscripts(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let (first_expression, second_expression) = if operator == BinaryOperator::Subtract {
            (right, left)
        } else {
            (left, right)
        };
        let (Some(first), Some(second)) = (
            self.resident_masked_subscript(first_expression),
            self.resident_masked_subscript(second_expression),
        ) else {
            return Ok(None);
        };
        // Member-derived indices add their own memory dependencies. This
        // schedule owns two register-derived offsets, with no calls or loads
        // hidden in either offset calculation.
        if first.index.loaded || second.index.loaded {
            return Ok(None);
        }
        let mut inputs = self.registers_used_by(left);
        inputs.extend(self.registers_used_by(right));
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            let primary = self.fresh_virtual_general_avoiding(inputs.into_iter().collect());
            self.with_reserved_inputs(second_expression, |me| {
                me.evaluate_general(first_expression, primary)
            })?;
            self.evaluate_general(second_expression, GENERAL_SCRATCH)?;
            return Ok(Some((primary, GENERAL_SCRATCH)));
        }

        if first.index.bias.is_some() || second.index.bias.is_some() {
            return self.place_biased_pointer_load_pair(&first, &second);
        }
        let first_source = self.general_register_of_leaf(first.index.source)?;
        let second_source = self.general_register_of_leaf(second.index.source)?;
        let first_offset = self.fresh_virtual_general_avoiding(inputs.into_iter().collect());
        let primary = self.fresh_virtual_general();
        self.emit_masked_scale(&second.index, second_source, GENERAL_SCRATCH);
        self.emit_masked_scale(&first.index, first_source, first_offset);
        let first_load = indexed_load(first.pointee, primary, first.address, first_offset)?;
        let second_load = indexed_load(
            second.pointee,
            GENERAL_SCRATCH,
            second.address,
            GENERAL_SCRATCH,
        )?;
        if self.behavior.computed_load_pair_secondary_first {
            self.output.instructions.extend([second_load, first_load]);
        } else {
            self.output.instructions.extend([first_load, second_load]);
        }
        Ok(Some((primary, GENERAL_SCRATCH)))
    }

    fn place_biased_pointer_load_pair(
        &mut self,
        first: &ResidentMaskedSubscript<'_>,
        second: &ResidentMaskedSubscript<'_>,
    ) -> Compilation<Option<(u8, u8)>> {
        let (biased, plain, biased_is_primary, immediate) =
            match (first.index.bias, second.index.bias) {
                (Some(value), None) => (first, second, true, value),
                (None, Some(value)) => (second, first, false, value),
                _ => return Ok(None),
            };
        let source = self.general_register_of_leaf(biased.index.source)?;
        let plain_source = self.general_register_of_leaf(plain.index.source)?;
        let primary = self.fresh_virtual_general_preferring(3);
        if biased_is_primary && self.behavior.biased_load_pair_primary_offset_first {
            let offset = self.fresh_virtual_general_preferring(5);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: source,
                immediate,
            });
            self.emit_masked_scale(&biased.index, GENERAL_SCRATCH, offset);
            self.emit_masked_scale(&plain.index, plain_source, GENERAL_SCRATCH);
            self.output.instructions.push(indexed_load(
                biased.pointee,
                primary,
                biased.address,
                offset,
            )?);
            self.output.instructions.push(indexed_load(
                plain.pointee,
                GENERAL_SCRATCH,
                plain.address,
                GENERAL_SCRATCH,
            )?);
            return Ok(Some((primary, GENERAL_SCRATCH)));
        }
        let temporary = if biased_is_primary {
            self.fresh_virtual_general_preferring(5)
        } else {
            GENERAL_SCRATCH
        };
        let (plain_value, biased_value) = if biased_is_primary {
            (GENERAL_SCRATCH, primary)
        } else {
            (primary, GENERAL_SCRATCH)
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: temporary,
            a: source,
            immediate,
        });
        self.emit_masked_scale(&plain.index, plain_source, plain_value);
        self.emit_masked_scale(&biased.index, temporary, biased_value);
        self.output.instructions.push(indexed_load(
            plain.pointee,
            plain_value,
            plain.address,
            plain_value,
        )?);
        self.output.instructions.push(indexed_load(
            biased.pointee,
            biased_value,
            biased.address,
            biased_value,
        )?);
        Ok(Some((primary, GENERAL_SCRATCH)))
    }

    pub(crate) fn place_indexed_load_pair(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if let Some(registers) = self.place_two_global_loads(operator, left, right)? {
            return Ok(Some(registers));
        }
        if let Some(registers) = self.place_shared_global_load_pair(operator, left, right)? {
            return Ok(Some(registers));
        }
        if let Some(registers) = self.place_masked_global_load_pair(operator, left, right)? {
            return Ok(Some(registers));
        }
        if let Some(registers) = self.place_two_masked_subscripts(operator, left, right)? {
            return Ok(Some(registers));
        }
        let (indexed, sibling, indexed_is_right) = if self.is_resident_displacement_load(right) {
            (left, right, false)
        } else if self.is_resident_displacement_load(left) {
            (right, left, true)
        } else {
            return Ok(None);
        };
        let Some(ResidentMaskedSubscript {
            pointee,
            address,
            index,
        }) = self.resident_masked_subscript(indexed)
        else {
            return Ok(None);
        };
        // Bias preparation beside a displacement load needs a separate issue order.
        if index.bias.is_some() {
            return Ok(None);
        }
        let optimized = self.behavior.optimization != mwcc_versions::Optimization::O0;
        let indexed_is_primary = operator == BinaryOperator::Subtract && indexed_is_right;
        let avoid = if !optimized {
            let mut inputs = self.registers_used_by(indexed);
            inputs.extend(self.registers_used_by(sibling));
            inputs.into_iter().collect()
        } else if self.behavior.scaled_load_pair_preserves_index_register
            && !index.loaded
            && index.shift != 0
            && !indexed_is_primary
        {
            self.registers_used_by(index.source).into_iter().collect()
        } else {
            Vec::new()
        };
        let primary = self.fresh_virtual_general_avoiding(avoid);
        let (value, other) = if indexed_is_primary {
            (primary, GENERAL_SCRATCH)
        } else {
            (GENERAL_SCRATCH, primary)
        };

        if !optimized {
            // O0 evaluates the machine operator's primary first. Keep the
            // sibling's pointer live even when the index reads the same object.
            let (first, second) = if indexed_is_primary {
                (indexed, sibling)
            } else {
                (sibling, indexed)
            };
            self.with_reserved_inputs(second, |me| me.evaluate_general(first, primary))?;
            self.evaluate_general(second, GENERAL_SCRATCH)?;
        } else {
            // Start the address chain, issue the independent load in its first
            // latency slot, then finish the indexed access. Each operand has its
            // own lane, including reverse subtraction where the sibling uses r0.
            let source = if index.loaded {
                // A loaded index dies at the scale. Its input register can
                // still be live for the sibling while the scaled result reuses
                // that register after the sibling has consumed it.
                let loaded = if indexed_is_primary {
                    self.fresh_virtual_general()
                } else {
                    value
                };
                self.with_reserved_inputs(sibling, |me| {
                    let reserved = me.reserved.insert(address);
                    let result = me.evaluate_general(index.source, loaded);
                    if reserved {
                        me.reserved.remove(&address);
                    }
                    result
                })?;
                loaded
            } else {
                self.general_register_of_leaf(index.source)?
            };
            if !index.loaded {
                self.emit_masked_scale(&index, source, value);
            }
            // The remaining address calculation uses the index lane and base;
            // the original source register can now be reused by this load.
            self.evaluate_general(sibling, other)?;
            if index.loaded {
                self.emit_masked_scale(&index, source, value);
            }
            self.output
                .instructions
                .push(indexed_load(pointee, value, address, value)?);
        }
        Ok(Some((primary, GENERAL_SCRATCH)))
    }
}
