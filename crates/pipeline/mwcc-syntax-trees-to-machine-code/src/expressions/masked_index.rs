//! Masked subscripts: select the mask together with the element scale, then
//! schedule it against the array address according to the index's source.

use super::*;

pub(super) struct MaskedIndex<'a> {
    pub(super) source: &'a Expression,
    mask: u32,
    pub(super) shift: u8,
    run: Option<(u8, u8)>,
    pub(super) loaded: bool,
}

impl Generator {
    pub(super) fn masked_index<'a>(&self, index: &'a Expression, pointee: Pointee) -> Option<MaskedIndex<'a>> {
        if !matches!(pointee.size(), 1 | 2 | 4) {
            return None;
        }
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = index
        else {
            return None;
        };
        let (source, mask) = if let Some(mask) = constant_value(right) {
            (left.as_ref(), mask as u32)
        } else {
            (right.as_ref(), constant_value(left)? as u32)
        };
        let loaded = match source {
            Expression::Variable(name)
                if self.locations.get(name).is_some_and(|v| v.width == 32) =>
            {
                false
            }
            Expression::Member {
                member_type:
                    Type::UnsignedChar | Type::UnsignedShort | Type::Int | Type::UnsignedInt,
                index_stride: None,
                ..
            } => true,
            _ => return None,
        };
        let shift = pointee.size().trailing_zeros() as u8;
        let run = mask_to_run(mask << shift);
        // Discontiguous immediate masks retain AND followed by the scale.
        if run.is_none() && (mask == 0 || mask > u16::MAX as u32) {
            return None;
        }
        if self.behavior.optimization == mwcc_versions::Optimization::O0
            && mask_to_run(mask).is_none()
            && mask > u16::MAX as u32
        {
            return None;
        }
        Some(MaskedIndex {
            source,
            mask,
            shift,
            run,
            loaded,
        })
    }

    fn emit_unscaled_index_mask(&mut self, index: &MaskedIndex<'_>, source: u8, destination: u8) {
        if let Some((begin, end)) = mask_to_run(index.mask) {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: destination,
                s: source,
                shift: 0,
                begin,
                end,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::AndImmediateRecord {
                    a: destination,
                    s: source,
                    immediate: index.mask as u16,
                });
        }
    }

    /// O0 consumes mask and scale before starting the address. The explicit
    /// address convention keeps the offset separate while r0 receives @lo.
    fn emit_unoptimized_masked_global(
        &mut self,
        name: &str,
        small: bool,
        pointee: Pointee,
        index: &MaskedIndex<'_>,
        destination: u8,
    ) -> Compilation<bool> {
        // O0 discovers a full array address before creating the function's
        // global symbol. SDA references retain the ordinary body event order.
        if !small {
            self.output.body_references_precede_symbol = true;
        }
        let explicit = !small
            && self.behavior.global_array_index_style
                == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress;
        let source = if index.loaded {
            self.evaluate_general(index.source, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(index.source)?
        };
        let scaled = if explicit {
            self.fresh_virtual_general_preferring(5)
        } else {
            GENERAL_SCRATCH
        };
        let masked = if index.shift == 0 {
            scaled
        } else {
            GENERAL_SCRATCH
        };
        self.emit_unscaled_index_mask(index, source, masked);
        if index.shift != 0 {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: masked,
                    shift: index.shift,
                });
        }
        let base = self.fresh_virtual_general_preferring(4);
        if small {
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: 0,
                immediate: 0,
            });
        } else {
            self.emit_address_high(base, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: if explicit { GENERAL_SCRATCH } else { base },
                a: base,
                immediate: 0,
            });
        }
        if explicit {
            self.output.instructions.push(Instruction::Add {
                d: base,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
            self.output
                .instructions
                .push(displacement_load(pointee, destination, base, 0)?);
        } else {
            self.output
                .instructions
                .push(indexed_load(pointee, destination, base, scaled)?);
        }
        Ok(true)
    }

    pub(super) fn emit_masked_scale(&mut self, index: &MaskedIndex<'_>, source: u8, destination: u8) {
        if let Some((begin, end)) = index.run {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: destination,
                s: source,
                shift: index.shift,
                begin,
                end,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::AndImmediateRecord {
                    a: destination,
                    s: source,
                    immediate: index.mask as u16,
                });
            if index.shift != 0 {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: destination,
                        s: destination,
                        shift: index.shift,
                    });
            }
        }
    }

    pub(crate) fn try_emit_masked_pointer_subscript(
        &mut self,
        pointee: Pointee,
        address: u8,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(index) = self.masked_index(index, pointee) else {
            return Ok(false);
        };
        if address == GENERAL_SCRATCH {
            return Ok(false);
        }
        let source = if index.loaded {
            let reserved = self.reserved.insert(address);
            self.evaluate_general(index.source, GENERAL_SCRATCH)?;
            if reserved {
                self.reserved.remove(&address);
            }
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(index.source)?
        };
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            self.emit_unscaled_index_mask(&index, source, GENERAL_SCRATCH);
            if index.shift != 0 {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        shift: index.shift,
                    });
            }
        } else {
            self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
        }
        self.output.instructions.push(indexed_load(
            pointee,
            destination,
            address,
            GENERAL_SCRATCH,
        )?);
        Ok(true)
    }

    pub(crate) fn try_emit_masked_global_subscript(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(mut index) = self.masked_index(expression, pointee) else {
            return Ok(false);
        };
        if self
            .data_section_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.symbols.contains(name))
        {
            return Ok(false);
        }
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            return self.emit_unoptimized_masked_global(name, small, pointee, &index, destination);
        }
        if small {
            let source = if index.loaded {
                self.evaluate_general(index.source, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            } else {
                self.general_register_of_leaf(index.source)?
            };
            let base = self.fresh_virtual_general_preferring(3);
            if !index.loaded {
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
            }
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: 0,
                immediate: 0,
            });
            if index.loaded {
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
            }
            self.output.instructions.push(indexed_load(
                pointee,
                destination,
                base,
                GENERAL_SCRATCH,
            )?);
            return Ok(true);
        }
        let mask_completed = !index.loaded && index.run.is_none();
        let legacy = self.behavior.global_array_index_style
            == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress;
        let source = if index.loaded {
            let source = if legacy {
                self.fresh_virtual_general_preferring(4)
            } else {
                GENERAL_SCRATCH
            };
            self.evaluate_general(index.source, source)?;
            source
        } else if index.run.is_none() {
            // A discontiguous parameter mask finishes before address setup.
            let source = if legacy {
                self.fresh_virtual_general_preferring(4)
            } else {
                GENERAL_SCRATCH
            };
            let parameter = self.general_register_of_leaf(index.source)?;
            self.output
                .instructions
                .push(Instruction::AndImmediateRecord {
                    a: source,
                    s: parameter,
                    immediate: index.mask as u16,
                });
            index.mask = u32::MAX;
            index.run = mask_to_run(u32::MAX << index.shift);
            source
        } else {
            self.general_register_of_leaf(index.source)?
        };
        let loaded = index.loaded || mask_completed;
        let high = self.fresh_virtual_general_preferring(if loaded { 3 } else { 4 });
        let address = if self.behavior.masked_global_index_retains_base {
            high
        } else {
            self.fresh_virtual_general_preferring(3)
        };
        self.emit_address_high(high, name);
        if legacy {
            if !loaded {
                self.emit_masked_scale(&index, source, address);
            }
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high,
                immediate: 0,
            });
            if loaded {
                self.emit_masked_scale(&index, source, address);
            }
            self.output.instructions.push(Instruction::Add {
                d: address,
                a: GENERAL_SCRATCH,
                b: address,
            });
            self.output
                .instructions
                .push(displacement_load(pointee, destination, address, 0)?);
        } else {
            if !loaded {
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
            }
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: address,
                a: high,
                immediate: 0,
            });
            if loaded {
                self.emit_masked_scale(&index, source, GENERAL_SCRATCH);
            }
            self.output.instructions.push(indexed_load(
                pointee,
                destination,
                address,
                GENERAL_SCRATCH,
            )?);
        }
        Ok(true)
    }
}
