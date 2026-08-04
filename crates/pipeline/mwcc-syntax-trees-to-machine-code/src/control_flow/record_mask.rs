//! Contiguous mask tests used directly by branches.
//!
//! Register leaves and single-word memory loads normally share the same terminal
//! `rlwinm.` test. Build 163 retains a distinct post-assembly optimizer state:
//! after an assembly function, it materializes the mask with non-recording ALU
//! instructions and compares the result against zero separately.

use super::*;

impl Generator {
    pub(super) fn try_emit_mask_condition_test(
        &mut self,
        value: &Expression,
        mask: &Expression,
    ) -> Compilation<bool> {
        // `if (bits & (1 << index))`: form the one-hot mask in r0 and
        // consume it with `and.` so the following branch reads CR0 directly.
        // This is the variable sibling of the constant rlwinm. path below.
        if let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } = mask
        {
            if constant_value(left) == Some(1) {
                if let (Some(value_register), Some(amount)) = (
                    leaf_name(value).and_then(|name| self.lookup_general(name)),
                    leaf_name(right).and_then(|name| self.lookup_general(name)),
                ) {
                    self.load_integer_constant(GENERAL_SCRATCH, 1);
                    self.output.instructions.push(Instruction::ShiftLeftWord {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        b: amount,
                    });
                    self.emit_condition_mask_and(value_register, GENERAL_SCRATCH);
                    self.emit_post_asm_mask_compare(value, GENERAL_SCRATCH)?;
                    return Ok(true);
                }
            }
        }
        let Some(mask) = constant_value(mask).and_then(|value| u32::try_from(value).ok()) else {
            return Ok(false);
        };
        let register_leaf = leaf_name(value).and_then(|name| self.lookup_general(name));
        let memory_value = self.is_word_load(value)
            || self.is_byte_load(value)
            || self.is_halfword_load(value)
            || self.is_global(value)
            || matches!(value, Expression::Variable(name)
                if self.frame_slots.get(name).is_some_and(|slot|
                    !slot.is_array && slot.class == ValueClass::General));
        if register_leaf.is_none() && !memory_value {
            return Ok(false);
        }
        if let Some((begin, end)) = mask_to_run(mask) {
            let source = if let Some(register) = register_leaf {
                register
            } else {
                self.evaluate_general(value, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            };
            if self.preceded_by_asm {
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: GENERAL_SCRATCH,
                    s: source,
                    shift: 0,
                    begin,
                    end,
                });
            } else {
                self.output.instructions.push(Instruction::AndMaskRecord {
                    a: GENERAL_SCRATCH,
                    s: source,
                    begin,
                    end,
                });
            }
            self.emit_post_asm_mask_compare(value, GENERAL_SCRATCH)?;
        } else {
            // A discontiguous wide mask cannot use rlwinm. MWCC forms it in
            // r0, keeps the loaded value in the next available register, and
            // lets `and.` both consume the mask and set CR0 for the branch.
            let low = mask as u16 as i16;
            let high = ((i64::from(mask) - i64::from(low)) >> 16) as i16;
            let high_register = self.fresh_virtual_general();
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(high_register, high));
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high_register,
                immediate: low,
            });
            let source = if let Some(register) = register_leaf {
                register
            } else {
                let register = self.fresh_virtual_general_preferring(5);
                self.evaluate_general(value, register)?;
                register
            };
            self.emit_condition_mask_and(source, GENERAL_SCRATCH);
            self.emit_post_asm_mask_compare(value, GENERAL_SCRATCH)?;
        }
        Ok(true)
    }

    fn emit_condition_mask_and(&mut self, source: u8, mask: u8) {
        let instruction = if self.preceded_by_asm {
            Instruction::And {
                a: GENERAL_SCRATCH,
                s: source,
                b: mask,
            }
        } else {
            Instruction::AndRecord {
                a: GENERAL_SCRATCH,
                s: source,
                b: mask,
            }
        };
        self.output.instructions.push(instruction);
    }

    fn emit_post_asm_mask_compare(
        &mut self,
        source: &Expression,
        result: u8,
    ) -> Compilation<()> {
        if !self.preceded_by_asm {
            return Ok(());
        }
        let promoted_signed = self
            .unpromoted_integer_width(source)
            .is_some_and(|width| width < 32)
            || self.signedness_of(source)?;
        let instruction = if promoted_signed {
            Instruction::CompareWordImmediate {
                a: result,
                immediate: 0,
            }
        } else {
            Instruction::CompareLogicalWordImmediate {
                a: result,
                immediate: 0,
            }
        };
        self.output.instructions.push(instruction);
        Ok(())
    }
}
