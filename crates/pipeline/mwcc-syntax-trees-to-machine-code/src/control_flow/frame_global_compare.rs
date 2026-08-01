//! Comparisons between a frame-resident scalar and an absolute global.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Type;

impl Generator {
    /// Keep two memory operands distinct while filling the absolute global's
    /// address latency: `lis base,global@ha; load frame; load global@l(base);
    /// cmp`.  The ordinary condition path uses r0 for either memory value and
    /// would otherwise overwrite the frame load with the global load.
    pub(crate) fn try_emit_frame_global_compare(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if self.behavior.global_addressing != mwcc_versions::GlobalAddressing::Absolute
            || self.behavior.absolute_access_style
                != mwcc_versions::AbsoluteAccessStyle::FoldedDisplacement
        {
            return Ok(None);
        }
        let Expression::Variable(frame_name) = left else {
            return Ok(None);
        };
        let Some(slot) = self
            .frame_slots
            .get(frame_name)
            .copied()
            .filter(|slot| !slot.is_array)
        else {
            return Ok(None);
        };
        let Expression::Variable(global_name) = right else {
            return Ok(None);
        };
        let Some(&global_type) = self.globals.get(global_name) else {
            return Ok(None);
        };
        if matches!(global_type, Type::Float | Type::Double | Type::Struct { .. }) {
            return Ok(None);
        }

        let global_base = self.fresh_virtual_general_preferring(3);
        let frame_value = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(global_base, global_name);
        self.evaluate_general(left, frame_value)?;
        self.record_relocation(mwcc_machine_code::RelocationKind::Addr16Lo, global_name);
        self.output.instructions.push(self.global_load_instruction(
            global_type,
            GENERAL_SCRATCH,
            global_base,
        )?);

        let signed = self.signed_of(slot.value_type) && self.signed_of(global_type);
        self.output.instructions.push(if signed {
            Instruction::CompareWord {
                a: frame_value,
                b: GENERAL_SCRATCH,
            }
        } else {
            Instruction::CompareLogicalWord {
                a: frame_value,
                b: GENERAL_SCRATCH,
            }
        });
        Ok(Some(
            false_branch_bo_bi(operator).expect("comparison operator was selected"),
        ))
    }
}
