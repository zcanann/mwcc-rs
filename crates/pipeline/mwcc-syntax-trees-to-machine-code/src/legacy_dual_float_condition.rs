//! Build-163 batching for two integer promotions in one float condition.

use crate::generator::{Generator, FLOAT_SCRATCH, GENERAL_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    /// Reserve `lanes` eight-byte conversion images beneath callee-saved homes.
    /// The linkage-first frame normalizer grows the physical frame after
    /// selection; ordinary frames retain the historical r1+8 scratch image.
    pub(crate) fn reserve_condition_conversion_scratch(&mut self, lanes: i16) -> i16 {
        if self.behavior.legacy_float_cast_schedule && !self.callee_saved.is_empty() {
            self.callee_saved_conversion_bytes =
                self.callee_saved_conversion_bytes.max(lanes * 8);
            self.frame_size
        } else {
            8
        }
    }

    /// Recognize `narrow_integer REL (word_integer + float)` and emit MWCC's
    /// paired magic-bias schedule. Both integers are converted from two adjacent
    /// stack images while the independent loads fill conversion latency slots.
    pub(crate) fn try_emit_legacy_dual_float_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        if !self.behavior.legacy_float_cast_schedule
            || double
            || !self.is_byte_load(left)
            || self.signedness_of(left)?
        {
            return Ok(None);
        }
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: add_left,
            right: add_right,
        } = right
        else {
            return Ok(None);
        };
        let (integer, float) = if self.is_word_load(add_left)
            && !self.is_float_operand(add_left)
            && self.is_float_operand(add_right)
        {
            (add_left.as_ref(), add_right.as_ref())
        } else if self.is_word_load(add_right)
            && !self.is_float_operand(add_right)
            && self.is_float_operand(add_left)
        {
            (add_right.as_ref(), add_left.as_ref())
        } else {
            return Ok(None);
        };
        if self.is_float_leaf(float) {
            return Ok(None);
        }

        let integer_signed = self.signedness_of(integer)?;
        self.evaluate_general(integer, GENERAL_SCRATCH)?;
        let high = 3;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(high, 17200));
        let narrow = 4;
        self.evaluate_general(left, narrow)?;
        if integer_signed {
            self.output
                .instructions
                .push(Instruction::XorImmediateShifted {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    immediate: 0x8000,
                });
        }

        let integer_bias_register = self.fresh_virtual_float_preferring(3);
        let narrow_bias_register = self.fresh_virtual_float_preferring(4);
        let integer_value_register = self.fresh_virtual_float_preferring(2);
        let narrow_value_register = self.fresh_virtual_float_preferring(3);
        let integer_bias = if integer_signed {
            0x4330_0000_8000_0000
        } else {
            0x4330_0000_0000_0000
        };
        let base = self.reserve_condition_conversion_scratch(2);
        self.load_double_constant(integer_bias_register, integer_bias);
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: 1,
            offset: base + 4,
        });
        self.load_double_constant(narrow_bias_register, 0x4330_0000_0000_0000);
        self.output.instructions.push(Instruction::StoreWord {
            s: high,
            a: 1,
            offset: base,
        });
        self.evaluate_float(float, FLOAT_SCRATCH)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: narrow,
            a: 1,
            offset: base + 12,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: integer_value_register,
            a: 1,
            offset: base,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: high,
            a: 1,
            offset: base + 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: integer_value_register,
                a: integer_value_register,
                b: integer_bias_register,
            });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: narrow_value_register,
            a: 1,
            offset: base + 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: narrow_value_register,
                a: narrow_value_register,
                b: narrow_bias_register,
            });
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: FLOAT_SCRATCH,
            a: integer_value_register,
            b: FLOAT_SCRATCH,
        });
        self.output.has_conversion = true;
        Ok(Some((narrow_value_register, FLOAT_SCRATCH)))
    }
}
