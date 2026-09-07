//! Place a masked global-array access beside an independent scalar-global load.

use super::masked_global_address::MaskedGlobalAddressUse;
use super::*;

impl Generator {
    pub(super) fn place_masked_global_load_pair(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let scalar_global = |expression: &Expression| {
            let Expression::Variable(name) = expression else {
                return false;
            };
            !self.locations.contains_key(name)
                && !self.frame_slots.contains_key(name)
                && !self.is_global_array(name)
                && matches!(
                    self.globals.get(name),
                    Some(
                        Type::UnsignedChar
                            | Type::Short
                            | Type::UnsignedShort
                            | Type::Int
                            | Type::UnsignedInt
                    )
                )
        };
        let (indexed, sibling, indexed_is_right) = if scalar_global(right) {
            (left, right, false)
        } else if scalar_global(left) {
            (right, left, true)
        } else {
            return Ok(None);
        };
        let Expression::Index { base, index } = indexed else {
            return Ok(None);
        };
        let Some(name) = leaf_name(base) else {
            return Ok(None);
        };
        let Some(total_size) = self.global_array_address_extent(name) else {
            return Ok(None);
        };
        // A small array has no high-half address issue slot. Absolute scalar
        // addressing has a second address chain; both need separate schedules.
        if total_size <= 8 || self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(None);
        }
        let Some(
            pointee @ (Pointee::UnsignedChar
            | Pointee::Short
            | Pointee::UnsignedShort
            | Pointee::Int
            | Pointee::UnsignedInt),
        ) = self.globals.get(name).copied().and_then(pointee_of_type)
        else {
            return Ok(None);
        };
        if self.masked_index(index, pointee).is_none()
            || self
                .data_section_anchor
                .as_ref()
                .is_some_and(|a| a.symbols.contains(name))
        {
            return Ok(None);
        }
        let optimized = self.behavior.optimization != mwcc_versions::Optimization::O0;
        let explicit = self.behavior.global_array_index_style
            == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress;
        let indexed_is_primary = operator == BinaryOperator::Subtract && indexed_is_right;
        let primary = if !optimized {
            self.fresh_virtual_general_preferring(if indexed_is_primary {
                4
            } else if explicit {
                6
            } else {
                5
            })
        } else {
            self.fresh_virtual_general()
        };
        let (value, other) = if indexed_is_primary {
            (primary, GENERAL_SCRATCH)
        } else {
            (GENERAL_SCRATCH, primary)
        };
        if !optimized {
            if let Expression::Variable(name) = sibling {
                if !self.output.body_value_references.contains(name) {
                    self.output.body_value_references.push(name.clone());
                }
            }
            let (first, second) = if indexed_is_primary {
                (indexed, sibling)
            } else {
                (sibling, indexed)
            };
            self.with_reserved_inputs(second, |me| me.evaluate_general(first, primary))?;
            self.evaluate_general(second, GENERAL_SCRATCH)?;
        } else {
            let address_use = MaskedGlobalAddressUse {
                preserve_scratch: indexed_is_primary,
                after_high: explicit.then_some((sibling, other)),
            };
            let address = self
                .select_masked_global_address(name, total_size, pointee, index, address_use)?
                .ok_or_else(|| {
                    Diagnostic::error("validated masked global address was not selected")
                })?;
            if !explicit {
                self.evaluate_general(sibling, other)?;
            }
            self.output.instructions.push(address.load(pointee, value)?);
        }
        Ok(Some((primary, GENERAL_SCRATCH)))
    }
}
