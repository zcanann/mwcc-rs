//! Build-163 comparisons whose consumer requires the result in `r0`.
//!
//! Stores and nested expressions use the general scratch as their destination.
//! The carry-chain selector therefore preserves its operands/results in ordinary
//! GPRs until the final boolean-producing instruction writes `r0`.

use crate::analysis::is_zero_literal;
use crate::generator::{Generator, GENERAL_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    pub(super) fn try_emit_legacy_scratch_comparison(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        signed: bool,
    ) -> Compilation<bool> {
        if operator == BinaryOperator::NotEqual && is_zero_literal(right) {
            let Some(source) = self
                .leaf_info(left)
                .ok()
                .filter(|&(_, width, _)| width == 32)
                .map(|(register, _, _)| register)
            else {
                return Ok(false);
            };
            self.output.instructions.push(Instruction::Negate {
                d: source,
                a: source,
            });
            self.emit_legacy_not_equal_tail(source, GENERAL_SCRATCH);
            return Ok(true);
        }

        if !signed {
            return Ok(false);
        }

        match operator {
            BinaryOperator::Less | BinaryOperator::Greater => {
                let left_leaf = self
                    .leaf_info(left)
                    .ok()
                    .filter(|&(_, width, _)| width == 32)
                    .map(|(register, _, _)| register);
                let right_leaf = self
                    .leaf_info(right)
                    .ok()
                    .filter(|&(_, width, _)| width == 32)
                    .map(|(register, _, _)| register);

                let (left_register, right_register, carry_result) =
                    if let (Some(left), Some(right)) = (left_leaf, right_leaf) {
                        (left, right, left)
                    } else if is_zero_literal(right) {
                        if let Some(left) = left_leaf {
                            let zero = self.fresh_virtual_general();
                            self.load_integer_constant(zero, 0);
                            (left, zero, left)
                        } else if self.is_simple_word_load(left) {
                            let value =
                                self.fresh_virtual_general_avoiding(self.load_base_registers(left));
                            self.evaluate_general(left, value)?;
                            let zero = self.fresh_virtual_general();
                            self.load_integer_constant(zero, 0);
                            (value, zero, zero)
                        } else {
                            return Ok(false);
                        }
                    } else {
                        return Ok(false);
                    };

                let (first, second) = if operator == BinaryOperator::Less {
                    (right_register, left_register)
                } else {
                    (left_register, right_register)
                };
                self.output.instructions.push(Instruction::Eqv {
                    a: GENERAL_SCRATCH,
                    s: first,
                    b: second,
                });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: carry_result,
                        a: first,
                        b: second,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::AddToZeroExtended {
                        d: GENERAL_SCRATCH,
                        a: GENERAL_SCRATCH,
                    });
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        clear: 31,
                    });
                Ok(true)
            }
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual if is_zero_literal(right) => {
                let Some(value) = self
                    .leaf_info(left)
                    .ok()
                    .filter(|&(_, width, _)| width == 32)
                    .map(|(register, _, _)| register)
                else {
                    return Ok(false);
                };
                self.load_integer_constant(GENERAL_SCRATCH, 0);
                let (high, low) = if operator == BinaryOperator::LessEqual {
                    (GENERAL_SCRATCH, value)
                } else {
                    (value, GENERAL_SCRATCH)
                };
                let sign_high = self.fresh_virtual_general();
                let sign_low = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: sign_high,
                        s: high,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: sign_low,
                        s: low,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: GENERAL_SCRATCH,
                        a: low,
                        b: high,
                    });
                self.output.instructions.push(Instruction::AddExtended {
                    d: GENERAL_SCRATCH,
                    a: sign_high,
                    b: sign_low,
                });
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn load_base_registers(&self, expression: &Expression) -> Vec<u8> {
        crate::expressions::load_base_name(expression)
            .and_then(|name| self.lookup_general(name))
            .into_iter()
            .collect()
    }
}
