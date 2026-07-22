//! Reassociated scaled sums with one structure-member leaf.
//!
//! CARD callback code computes bit counts such as
//! `((length + bias + record->latency) * scale) + tail`. MWCC groups the two
//! register leaves before applying the inner constant, then strength-reduces the
//! scale. Keeping that topology here avoids broadening the ordinary add-tree
//! evaluator with a memory-bearing special case.

use super::*;

impl Generator {
    pub(crate) fn try_emit_biased_scaled_member_sum(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: product,
            right: tail,
        } = expression
        else {
            return Ok(false);
        };
        let Some(tail) = constant_value(tail).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: sum,
            right: scale,
        } = product.as_ref()
        else {
            return Ok(false);
        };
        let Some(scale) = constant_value(scale).and_then(|value| u32::try_from(value).ok()) else {
            return Ok(false);
        };
        if scale < 2 || !scale.is_power_of_two() {
            return Ok(false);
        }
        let shift = scale.trailing_zeros();
        if shift > 31 {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: biased,
            right: member,
        } = sum.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: variable,
            right: bias,
        } = biased.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(variable) = variable.as_ref() else {
            return Ok(false);
        };
        let Some(variable) = self.lookup_general(variable) else {
            return Ok(false);
        };
        let Some(bias) = constant_value(bias).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        let Expression::Member {
            member_type: member_type @ (Type::Int | Type::UnsignedInt),
            index_stride: None,
            ..
        } = member.as_ref()
        else {
            return Ok(false);
        };

        self.evaluate(member, *member_type, GENERAL_SCRATCH)?;
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: variable,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: destination,
            immediate: bias,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: destination,
                s: GENERAL_SCRATCH,
                shift: shift as u8,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: destination,
            immediate: tail,
        });
        Ok(true)
    }
}
