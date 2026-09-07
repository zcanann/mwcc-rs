//! Shared address construction for masked file-scope array accesses.
//! Keeping the final load separate lets a binary operand fill the address
//! latency slot without duplicating mask, relocation, or version selection.

use super::masked_index::MaskedIndex;
use super::*;

pub(super) enum MaskedGlobalAddress {
    Displacement { base: u8 },
    Indexed { base: u8, offset: u8 },
}

impl MaskedGlobalAddress {
    pub(super) fn load(self, pointee: Pointee, destination: u8) -> Compilation<Instruction> {
        match self {
            Self::Displacement { base } => displacement_load(pointee, destination, base, 0),
            Self::Indexed { base, offset } => indexed_load(pointee, destination, base, offset),
        }
    }
}

#[derive(Default)]
pub(super) struct MaskedGlobalAddressUse<'a> {
    /// Optional high-half and completed-base registers for O0 operand placement.
    pub(super) unoptimized_base: Option<(u8, u8)>,
    /// A sibling value occupies r0 before this address's final load.
    pub(super) preserve_scratch: bool,
    /// An independent load can fill the first high-half address latency slot.
    pub(super) after_high: Option<(&'a Expression, u8)>,
}

impl Generator {
    pub(crate) fn try_emit_masked_global_subscript(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(address) = self.select_masked_global_address(
            name,
            total_size,
            pointee,
            expression,
            MaskedGlobalAddressUse::default(),
        )?
        else {
            return Ok(false);
        };
        self.output
            .instructions
            .push(address.load(pointee, destination)?);
        Ok(true)
    }

    fn emit_unoptimized_masked_global_address(
        &mut self,
        name: &str,
        small: bool,
        index: &MaskedIndex<'_>,
        registers: Option<(u8, u8)>,
    ) -> Compilation<MaskedGlobalAddress> {
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
        let (high, base) = registers.unwrap_or_else(|| {
            let base = self.fresh_virtual_general_preferring(4);
            (base, base)
        });
        if small {
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: 0,
                immediate: 0,
            });
        } else {
            self.emit_address_high(high, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: if explicit { GENERAL_SCRATCH } else { base },
                a: high,
                immediate: 0,
            });
        }
        if explicit {
            self.output.instructions.push(Instruction::Add {
                d: base,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
            Ok(MaskedGlobalAddress::Displacement { base })
        } else {
            Ok(MaskedGlobalAddress::Indexed {
                base,
                offset: scaled,
            })
        }
    }

    pub(super) fn select_masked_global_address(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        expression: &Expression,
        address_use: MaskedGlobalAddressUse<'_>,
    ) -> Compilation<Option<MaskedGlobalAddress>> {
        let Some(mut index) = self.masked_index(expression, pointee) else {
            return Ok(None);
        };
        if self
            .data_section_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.symbols.contains(name))
        {
            return Ok(None);
        }
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            return self
                .emit_unoptimized_masked_global_address(
                    name,
                    small,
                    &index,
                    address_use.unoptimized_base,
                )
                .map(Some);
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
            return Ok(Some(MaskedGlobalAddress::Indexed {
                base,
                offset: GENERAL_SCRATCH,
            }));
        }
        let mask_completed = !index.loaded && index.run.is_none();
        let legacy = self.behavior.global_array_index_style
            == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress;
        let source = if index.loaded {
            let source = if legacy || address_use.preserve_scratch {
                self.fresh_virtual_general_preferring(if legacy { 4 } else { 5 })
            } else {
                GENERAL_SCRATCH
            };
            self.evaluate_general(index.source, source)?;
            source
        } else if index.run.is_none() {
            // A discontiguous parameter mask finishes before address setup.
            let source = if legacy || address_use.preserve_scratch {
                self.fresh_virtual_general_preferring(if legacy { 4 } else { 5 })
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
        let offset = if address_use.preserve_scratch {
            self.fresh_virtual_general_preferring(
                if !legacy && self.behavior.masked_global_index_retains_base {
                    3
                } else {
                    5
                },
            )
        } else if legacy {
            address
        } else {
            GENERAL_SCRATCH
        };
        let low = if legacy && !address_use.preserve_scratch {
            GENERAL_SCRATCH
        } else {
            address
        };
        self.emit_address_high(high, name);
        if let Some((expression, destination)) = address_use.after_high {
            self.evaluate_general(expression, destination)?;
        }
        if legacy {
            if !loaded {
                self.emit_masked_scale(&index, source, offset);
            }
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: low,
                a: high,
                immediate: 0,
            });
            if loaded {
                self.emit_masked_scale(&index, source, offset);
            }
            self.output.instructions.push(Instruction::Add {
                d: address,
                a: low,
                b: offset,
            });
            Ok(Some(MaskedGlobalAddress::Displacement { base: address }))
        } else {
            if !loaded {
                self.emit_masked_scale(&index, source, offset);
            }
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: address,
                a: high,
                immediate: 0,
            });
            if loaded {
                self.emit_masked_scale(&index, source, offset);
            }
            Ok(Some(MaskedGlobalAddress::Indexed {
                base: address,
                offset,
            }))
        }
    }
}
