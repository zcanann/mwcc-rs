//! A masked word subscript and element zero sharing one full global-array base.
//! Versioned placement owns the address/load combination; mask selection and
//! O0 computed addresses use the existing shared builders.

use super::masked_global_address::MaskedGlobalAddressUse;
use super::*;
use mwcc_versions::SharedGlobalLoadStyle;

fn zero_subscript_base(expression: &Expression) -> Option<&str> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    (constant_value(index) == Some(0))
        .then(|| leaf_name(base))
        .flatten()
}

impl Generator {
    fn emit_unoptimized_shared_constant(
        &mut self,
        name: &str,
        pointee: Pointee,
        destination: u32,
        base_preference: u32,
    ) -> Compilation<()> {
        let base = self.fresh_virtual_general_preferring(base_preference);
        self.emit_address_high(base, name);
        if matches!(
            self.behavior.shared_global_load_style,
            SharedGlobalLoadStyle::SeparateLowRelocations { .. }
        ) {
            self.record_relocation(RelocationKind::Addr16Lo, name);
        } else {
            self.emit_address_low(base, name);
        }
        self.output
            .instructions
            .push(displacement_load(pointee, destination, base, 0)?);
        Ok(())
    }

    pub(super) fn place_shared_global_load_pair(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u32, u32)>> {
        let (name, indexed, indexed_is_right) = if let Some(name) = zero_subscript_base(right) {
            (name, left, false)
        } else if let Some(name) = zero_subscript_base(left) {
            (name, right, true)
        } else {
            return Ok(None);
        };
        let Expression::Index {
            base,
            index: expression,
        } = indexed
        else {
            return Ok(None);
        };
        if leaf_name(base) != Some(name) {
            return Ok(None);
        }
        let Some(total_size) = self
            .global_array_address_extent(name)
            .filter(|size| *size > 8)
        else {
            return Ok(None);
        };
        let Some(pointee @ (Pointee::Int | Pointee::UnsignedInt)) =
            self.globals.get(name).copied().and_then(pointee_of_type)
        else {
            return Ok(None);
        };
        let Some(index) = self.masked_index(expression, pointee) else {
            return Ok(None);
        };
        // A member-derived index introduces another load dependency. Keep it
        // with the other owners until that issue order has been characterized.
        if index.loaded
            || self
                .data_section_anchor
                .as_ref()
                .is_some_and(|a| a.symbols.contains(name))
        {
            return Ok(None);
        }
        let style = self.behavior.shared_global_load_style;
        let indexed_is_primary = operator == BinaryOperator::Subtract && indexed_is_right;
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            let reuse_address = indexed_is_primary
                && matches!(
                    style,
                    SharedGlobalLoadStyle::SeparateLowRelocations {
                        unoptimized_primary_reuses_address: true
                    }
                );
            let primary = self.fresh_virtual_general_preferring(if reuse_address {
                4
            } else if indexed_is_primary
                || matches!(style, SharedGlobalLoadStyle::SeparateLowRelocations { .. })
            {
                5
            } else {
                6
            });
            if !indexed_is_primary {
                self.emit_unoptimized_shared_constant(name, pointee, primary, 4)?;
            }
            let unoptimized_base =
                reuse_address.then(|| (self.fresh_virtual_general_preferring(5), primary));
            let address = self
                .select_masked_global_address(
                    name,
                    total_size,
                    pointee,
                    expression,
                    MaskedGlobalAddressUse {
                        unoptimized_base,
                        ..Default::default()
                    },
                )?
                .ok_or_else(|| {
                    Diagnostic::error("validated shared global address was not selected")
                })?;
            self.output.instructions.push(address.load(
                pointee,
                if indexed_is_primary {
                    primary
                } else {
                    GENERAL_SCRATCH
                },
            )?);
            if indexed_is_primary {
                self.emit_unoptimized_shared_constant(
                    name,
                    pointee,
                    GENERAL_SCRATCH,
                    if reuse_address { 5 } else { 4 },
                )?;
            }
            return Ok(Some((primary, GENERAL_SCRATCH)));
        }

        let source = self.general_register_of_leaf(index.source)?;
        let high = self.fresh_virtual_general_preferring(4);
        let primary = self.fresh_virtual_general_preferring(
            if indexed_is_primary || style == SharedGlobalLoadStyle::UpdatingBaseLoad {
                3
            } else {
                4
            },
        );
        self.emit_address_high(high, name);
        match style {
            SharedGlobalLoadStyle::ExplicitElementAddress if indexed_is_primary => {
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output
                    .instructions
                    .push(Instruction::LoadWordWithUpdate {
                        d: GENERAL_SCRATCH,
                        a: high,
                        offset: 0,
                    });
                self.emit_masked_scale(&index, source, primary);
                self.output.instructions.push(Instruction::Add {
                    d: primary,
                    a: high,
                    b: primary,
                });
                self.output
                    .instructions
                    .push(displacement_load(pointee, primary, primary, 0)?);
            }
            SharedGlobalLoadStyle::ExplicitElementAddress => {
                let base = self.fresh_virtual_general_preferring(5);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: high,
                    immediate: 0,
                });
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
                self.output
                    .instructions
                    .push(displacement_load(pointee, primary, base, 0)?);
                let element = self.fresh_virtual_general_preferring(3);
                self.output.instructions.push(Instruction::Add {
                    d: element,
                    a: base,
                    b: GENERAL_SCRATCH,
                });
                self.output.instructions.push(displacement_load(
                    pointee,
                    GENERAL_SCRATCH,
                    element,
                    0,
                )?);
            }
            SharedGlobalLoadStyle::UpdatingBaseLoad => {
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
                if indexed_is_primary {
                    self.emit_address_low(high, name);
                    self.output.instructions.push(indexed_load(
                        pointee,
                        primary,
                        high,
                        GENERAL_SCRATCH,
                    )?);
                    self.output.instructions.push(displacement_load(
                        pointee,
                        GENERAL_SCRATCH,
                        high,
                        0,
                    )?);
                } else {
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                    self.output
                        .instructions
                        .push(Instruction::LoadWordWithUpdate {
                            d: primary,
                            a: high,
                            offset: 0,
                        });
                    self.output.instructions.push(indexed_load(
                        pointee,
                        GENERAL_SCRATCH,
                        high,
                        GENERAL_SCRATCH,
                    )?);
                }
            }
            SharedGlobalLoadStyle::SeparateLowRelocations { .. } => {
                let offset = if indexed_is_primary {
                    self.fresh_virtual_general_preferring(5)
                } else {
                    GENERAL_SCRATCH
                };
                let base = self.fresh_virtual_general_preferring(3);
                self.emit_masked_scale(&index, source, offset);
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: high,
                    immediate: 0,
                });
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(displacement_load(
                    pointee,
                    if indexed_is_primary {
                        GENERAL_SCRATCH
                    } else {
                        primary
                    },
                    high,
                    0,
                )?);
                self.output.instructions.push(indexed_load(
                    pointee,
                    if indexed_is_primary {
                        primary
                    } else {
                        GENERAL_SCRATCH
                    },
                    base,
                    offset,
                )?);
            }
        }
        Ok(Some((primary, GENERAL_SCRATCH)))
    }
}
