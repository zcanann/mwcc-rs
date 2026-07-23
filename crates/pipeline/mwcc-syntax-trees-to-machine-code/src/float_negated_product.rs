//! A directly-negated memory value multiplied by another memory value.
//!
//! This is not the fused-negative multiply/add family. MWCC loads both values,
//! negates the source-side value in its allocated home, then emits an ordinary
//! multiply into `f0`. Keeping the negated lifetime virtual preserves a live
//! `f1` when one exists and still colors to `f1` in the measured leaf.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, UnaryOperator};

impl Generator {
    pub(crate) fn try_emit_negated_located_product(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: negated,
        } = left
        else {
            return Ok(false);
        };
        if destination != FLOAT_SCRATCH
            || !self.is_float_located(negated)
            || !self.is_float_located(right)
        {
            return Ok(false);
        }

        let negated_home = self.fresh_virtual_float_preferring(1);
        self.emit_located_operand(negated, negated_home)?;
        self.emit_located_operand(right, FLOAT_SCRATCH)?;
        self.output.instructions.push(Instruction::FloatNegate {
            d: negated_home,
            b: negated_home,
        });
        self.output.instructions.push(if double {
            Instruction::FloatMultiplyDouble {
                d: destination,
                a: negated_home,
                c: FLOAT_SCRATCH,
            }
        } else {
            Instruction::FloatMultiplySingle {
                d: destination,
                a: negated_home,
                c: FLOAT_SCRATCH,
            }
        });
        Ok(true)
    }
}
