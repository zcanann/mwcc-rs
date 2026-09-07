//! Masked subscripts: select the mask together with the element scale, then
//! schedule it against the array address according to the index's source.

use super::*;

pub(super) struct MaskedIndex<'a> {
    pub(super) source: &'a Expression,
    pub(super) mask: u32,
    pub(super) shift: u8,
    pub(super) run: Option<(u8, u8)>,
    pub(super) loaded: bool,
}

impl Generator {
    pub(super) fn masked_index<'a>(
        &self,
        index: &'a Expression,
        pointee: Pointee,
    ) -> Option<MaskedIndex<'a>> {
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

    pub(super) fn emit_unscaled_index_mask(
        &mut self,
        index: &MaskedIndex<'_>,
        source: u8,
        destination: u8,
    ) {
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

    pub(super) fn emit_masked_scale(
        &mut self,
        index: &MaskedIndex<'_>,
        source: u8,
        destination: u8,
    ) {
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
}
