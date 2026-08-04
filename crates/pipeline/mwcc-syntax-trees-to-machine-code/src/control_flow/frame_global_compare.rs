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
        let (frame_name, compare_address) = match left {
            Expression::Variable(name) => (name, false),
            Expression::AddressOf { operand } => match operand.as_ref() {
                Expression::Variable(name) => (name, true),
                _ => return Ok(None),
            },
            _ => return Ok(None),
        };
        let Some(slot) = self
            .frame_slots
            .get(frame_name)
            .copied()
            .filter(|slot| compare_address || !slot.is_array)
        else {
            return Ok(None);
        };
        if compare_address {
            if let Expression::Dereference { pointer } = right {
                if let Some((pointee, address)) =
                    crate::expressions::const_address_pointer(pointer)
                {
                    let (high, low) = crate::expressions::split_address(address);
                    let fixed_base = if high == 0 {
                        0
                    } else {
                        let Some((base, materialize)) =
                            self.claim_const_address_base_avoiding(high, Vec::new())
                        else {
                            return Ok(None);
                        };
                        if materialize {
                            self.output
                                .instructions
                                .push(Instruction::load_immediate_shifted(base, high));
                        }
                        base
                    };
                    let frame_address =
                        self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
                    self.prefer_virtual_general(frame_address, 4);
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: frame_address,
                        a: 1,
                        immediate: slot.offset,
                    });
                    self.output.instructions.push(crate::expressions::displacement_load(
                        pointee,
                        GENERAL_SCRATCH,
                        fixed_base,
                        low,
                    )?);
                    self.output
                        .instructions
                        .push(Instruction::CompareLogicalWord {
                            a: frame_address,
                            b: GENERAL_SCRATCH,
                        });
                    return Ok(Some(
                        false_branch_bo_bi(operator)
                            .expect("comparison operator was selected"),
                    ));
                }
            }
        }
        if self.behavior.global_addressing != mwcc_versions::GlobalAddressing::Absolute
            || self.behavior.absolute_access_style
                != mwcc_versions::AbsoluteAccessStyle::FoldedDisplacement
        {
            return Ok(None);
        }
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
        let frame_value = self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
        self.prefer_virtual_general(frame_value, 4);
        self.emit_address_high(global_base, global_name);
        if compare_address {
            self.output.instructions.push(Instruction::AddImmediate {
                d: frame_value,
                a: 1,
                immediate: slot.offset,
            });
        } else {
            self.evaluate_general(left, frame_value)?;
        }
        self.record_relocation(mwcc_machine_code::RelocationKind::Addr16Lo, global_name);
        self.output.instructions.push(self.global_load_instruction(
            global_type,
            GENERAL_SCRATCH,
            global_base,
        )?);

        let signed = !compare_address
            && self.signed_of(slot.value_type)
            && self.signed_of(global_type);
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
