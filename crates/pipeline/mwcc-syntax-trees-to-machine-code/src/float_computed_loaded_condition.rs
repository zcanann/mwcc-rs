//! Floating comparisons between a computed value and a direct memory value.
//!
//! The computed side is consumed immediately by `fcmp*`, so MWCC gives it the
//! volatile `f0` scratch.  The independent memory side joins register
//! allocation: it may reuse a dead parameter home, while live parameters and
//! retained condition values remain pinned.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, UnaryOperator};

impl Generator {
    pub(crate) fn try_place_computed_left_loaded_float_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !matches!(
            left,
            Expression::Binary { .. }
                | Expression::Unary { .. }
                | Expression::Cast { .. }
                | Expression::Conditional { .. }
        ) {
            return Ok(None);
        }

        if let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } = right
        {
            if self.is_float_located(operand) {
                self.place_condition_float_load(operand, FLOAT_SCRATCH)?;
                let computed = self.fresh_virtual_float_preferring(1);
                self.evaluate_float(left, computed)?;
                self.output.instructions.push(Instruction::FloatNegate {
                    d: FLOAT_SCRATCH,
                    b: FLOAT_SCRATCH,
                });
                return Ok(Some((computed, FLOAT_SCRATCH)));
            }
        }

        if !self.is_float_located(right) {
            return Ok(None);
        }
        self.evaluate_float(left, FLOAT_SCRATCH)?;
        let loaded_home = self.fresh_virtual_float_preferring(1);
        let loaded = self.place_condition_float_load(right, loaded_home)?;
        Ok(Some((FLOAT_SCRATCH, loaded)))
    }
}
