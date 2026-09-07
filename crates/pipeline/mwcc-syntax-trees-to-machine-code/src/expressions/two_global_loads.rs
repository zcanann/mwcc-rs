//! Two masked array reads with distinct global bases. Keep their address phases
//! visible together so versioned placement can overlap the independent chains.

use super::masked_index::MaskedIndex;
use super::*;
use mwcc_versions::GlobalLoadPairStyle;

pub(super) struct GlobalMaskedLoad<'a> {
    pub(super) name: &'a str,
    pub(super) total_size: u32,
    pub(super) pointee: Pointee,
    pub(super) index: MaskedIndex<'a>,
}

impl Generator {
    pub(super) fn global_masked_load<'a>(
        &self,
        expression: &'a Expression,
    ) -> Option<GlobalMaskedLoad<'a>> {
        let Expression::Index { base, index } = expression else {
            return None;
        };
        let name = leaf_name(base)?;
        let total_size = self.global_array_address_extent(name)?;
        if total_size <= 8 {
            return None;
        }
        let pointee = self.globals.get(name).copied().and_then(pointee_of_type)?;
        if !matches!(
            pointee,
            Pointee::UnsignedChar
                | Pointee::Short
                | Pointee::UnsignedShort
                | Pointee::Int
                | Pointee::UnsignedInt
        ) {
            return None;
        }
        let index = self.masked_global_lookup_index(index, pointee)?;
        // Loaded indices add memory dependencies to the two address chains.
        if index.loaded {
            return None;
        }
        Some(GlobalMaskedLoad {
            name,
            total_size,
            pointee,
            index,
        })
    }

    pub(super) fn place_two_global_loads(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let (first_expr, second_expr) = if operator == BinaryOperator::Subtract {
            (right, left)
        } else {
            (left, right)
        };
        let (Some(first), Some(second)) = (
            self.global_masked_load(first_expr),
            self.global_masked_load(second_expr),
        ) else {
            return Ok(None);
        };
        if first.name == second.name
            || self
                .data_section_anchor
                .as_ref()
                .is_some_and(|a| a.symbols.contains(first.name) || a.symbols.contains(second.name))
        {
            return Ok(None);
        }
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            let explicit = self.behavior.global_array_index_style
                == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress;
            let primary = self.fresh_virtual_general_preferring(if explicit { 6 } else { 5 });
            self.with_reserved_inputs(second_expr, |me| me.evaluate_general(first_expr, primary))?;
            self.evaluate_general(second_expr, GENERAL_SCRATCH)?;
            return Ok(Some((primary, GENERAL_SCRATCH)));
        }

        let style = self.behavior.global_load_pair_style;
        let source1 = self.general_register_of_leaf(first.index.source)?;
        let source2 = self.general_register_of_leaf(second.index.source)?;
        let base1 = self.fresh_virtual_general_preferring(5);
        let high2 = self.fresh_virtual_general_preferring(4);
        let offset1 = self.fresh_virtual_general_preferring(6);
        let primary = self.fresh_virtual_general_preferring(match style {
            GlobalLoadPairStyle::SerialExplicit => 5,
            GlobalLoadPairStyle::IndexedRetainedBases => 3,
            _ => 4,
        });
        self.emit_address_high(base1, first.name);
        if style == GlobalLoadPairStyle::SerialExplicit {
            self.emit_masked_scale(&first.index, source1, offset1);
            self.record_relocation(RelocationKind::Addr16Lo, first.name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: base1,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: base1,
                a: GENERAL_SCRATCH,
                b: offset1,
            });
            self.emit_address_high(high2, second.name);
            self.output
                .instructions
                .push(displacement_load(first.pointee, primary, base1, 0)?);
            let element2 = self.fresh_virtual_general_preferring(3);
            self.emit_masked_scale(&second.index, source2, element2);
            self.record_relocation(RelocationKind::Addr16Lo, second.name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high2,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: element2,
                a: GENERAL_SCRATCH,
                b: element2,
            });
            self.output.instructions.push(displacement_load(
                second.pointee,
                GENERAL_SCRATCH,
                element2,
                0,
            )?);
        } else if style == GlobalLoadPairStyle::ParallelExplicit {
            self.emit_address_high(high2, second.name);
            self.emit_address_low(base1, first.name);
            self.emit_masked_scale(&first.index, source1, offset1);
            self.record_relocation(RelocationKind::Addr16Lo, second.name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high2,
                immediate: 0,
            });
            let element2 = self.fresh_virtual_general_preferring(3);
            self.emit_masked_scale(&second.index, source2, element2);
            self.output.instructions.push(Instruction::Add {
                d: primary,
                a: base1,
                b: offset1,
            });
            self.output.instructions.push(Instruction::Add {
                d: element2,
                a: GENERAL_SCRATCH,
                b: element2,
            });
            self.output
                .instructions
                .push(displacement_load(first.pointee, primary, primary, 0)?);
            self.output.instructions.push(displacement_load(
                second.pointee,
                GENERAL_SCRATCH,
                element2,
                0,
            )?);
        } else {
            self.emit_address_high(high2, second.name);
            self.emit_masked_scale(&first.index, source1, offset1);
            self.emit_masked_scale(&second.index, source2, GENERAL_SCRATCH);
            let base2 = if style == GlobalLoadPairStyle::IndexedRetainedBases {
                self.emit_address_low(base1, first.name);
                self.emit_address_low(high2, second.name);
                high2
            } else {
                let base2 = self.fresh_virtual_general_preferring(3);
                self.record_relocation(RelocationKind::Addr16Lo, second.name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base2,
                    a: high2,
                    immediate: 0,
                });
                self.emit_address_low(base1, first.name);
                base2
            };
            self.output
                .instructions
                .push(indexed_load(first.pointee, primary, base1, offset1)?);
            self.output.instructions.push(indexed_load(
                second.pointee,
                GENERAL_SCRATCH,
                base2,
                GENERAL_SCRATCH,
            )?);
        }
        Ok(Some((primary, GENERAL_SCRATCH)))
    }
}
