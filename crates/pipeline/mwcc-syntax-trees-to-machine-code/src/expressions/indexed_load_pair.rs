//! Operand placement for a masked pointer subscript beside a displacement load.
//! The arithmetic emitter owns the final operator; mask selection stays shared
//! with ordinary subscripts. This owner only selects lifetimes and issue order.

use super::*;

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

    pub(crate) fn place_indexed_load_pair(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let (indexed, sibling, indexed_is_right) = if self.is_resident_displacement_load(right) {
            (left, right, false)
        } else if self.is_resident_displacement_load(left) {
            (right, left, true)
        } else {
            return Ok(None);
        };
        let Expression::Index { base, index } = indexed else {
            return Ok(None);
        };
        let Some(location) = leaf_name(base)
            .filter(|name| !self.frame_slots.contains_key(*name))
            .and_then(|name| self.locations.get(name))
        else {
            return Ok(None);
        };
        let Some(
            pointee @ (Pointee::UnsignedChar
            | Pointee::Short
            | Pointee::UnsignedShort
            | Pointee::Int
            | Pointee::UnsignedInt),
        ) = location.pointee
        else {
            return Ok(None);
        };
        let address = location.register;
        if address == GENERAL_SCRATCH {
            return Ok(None);
        }
        let Some(index) = self.masked_index(index, pointee) else {
            return Ok(None);
        };
        if index.loaded && !self.is_resident_displacement_load(index.source) {
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
