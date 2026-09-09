//! Masked subscripts: select the mask together with the element scale, then
//! schedule it against the array address according to the index's source.

use super::*;

pub(super) struct MaskedIndex<'a> {
    pub(super) source: &'a Expression,
    /// Optional input addition before masking; selected only by bias-aware callers.
    pub(super) bias: Option<i16>,
    pub(super) mask: u32,
    pub(super) shift: u8,
    /// Fused input rotation: element scale minus an optional right shift.
    pub(super) rotate: u8,
    pub(super) run: Option<(u8, u8)>,
    pub(super) loaded: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MaskedInputForm {
    Plain,
    BiasedPointer,
    ShiftedGlobal,
}

impl Generator {
    pub(super) fn masked_index<'a>(
        &self,
        index: &'a Expression,
        pointee: Pointee,
    ) -> Option<MaskedIndex<'a>> {
        self.select_masked_index(index, pointee, MaskedInputForm::Plain)
    }

    pub(super) fn masked_index_with_bias<'a>(
        &self,
        index: &'a Expression,
        pointee: Pointee,
    ) -> Option<MaskedIndex<'a>> {
        self.select_masked_index(index, pointee, MaskedInputForm::BiasedPointer)
    }

    pub(super) fn masked_global_lookup_index<'a>(
        &self,
        index: &'a Expression,
        pointee: Pointee,
    ) -> Option<MaskedIndex<'a>> {
        self.select_masked_index(index, pointee, MaskedInputForm::ShiftedGlobal)
    }

    fn select_masked_index<'a>(
        &self,
        index: &'a Expression,
        pointee: Pointee,
        input_form: MaskedInputForm,
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
        let (source, bias) = if input_form == MaskedInputForm::BiasedPointer {
            match source {
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } => {
                    let (source, value) = if let Some(value) = constant_value(right) {
                        (left.as_ref(), value)
                    } else {
                        (right.as_ref(), constant_value(left)?)
                    };
                    // A memory-derived input needs its own load issue policy.
                    if !matches!(source, Expression::Variable(_)) {
                        return None;
                    }
                    (source, Some(i16::try_from(value).ok()?))
                }
                _ => (source, None),
            }
        } else {
            (source, None)
        };
        let (source, right_shift) = if input_form == MaskedInputForm::ShiftedGlobal {
            match source {
                Expression::Binary {
                    operator: BinaryOperator::ShiftRight,
                    left,
                    right,
                } => {
                    let shift = u8::try_from(constant_value(right)?).ok()?;
                    if shift >= 32 || mask & !(u32::MAX >> shift) != 0 {
                        return None;
                    }
                    (left.as_ref(), shift)
                }
                _ => (source, 0),
            }
        } else {
            (source, 0)
        };
        let loaded = match source {
            Expression::Variable(name)
                if self.locations.get(name).is_some_and(|v| {
                    v.width == 32
                        || (input_form == MaskedInputForm::ShiftedGlobal
                            && !v.signed
                            && matches!(v.width, 8 | 16)
                            && ((mask as u64) << right_shift) < (1u64 << v.width))
                }) =>
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
        // Shifted discontiguous masks need an additional preparation phase.
        if right_shift != 0 && run.is_none() {
            return None;
        }
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
            bias,
            mask,
            shift,
            rotate: (shift + 32 - right_shift) & 31,
            run,
            loaded,
        })
    }

    pub(super) fn emit_unscaled_index_mask(
        &mut self,
        index: &MaskedIndex<'_>,
        source: u32,
        destination: u32,
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
        source: u32,
        destination: u32,
    ) {
        if let Some((begin, end)) = index.run {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: destination,
                s: source,
                shift: index.rotate,
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
        address: u32,
        index: &Expression,
        destination: u32,
    ) -> Compilation<bool> {
        let Some(index) = self.masked_index_with_bias(index, pointee) else {
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
        let source = if let Some(immediate) = index.bias {
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: source,
                immediate,
            });
            GENERAL_SCRATCH
        } else {
            source
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
