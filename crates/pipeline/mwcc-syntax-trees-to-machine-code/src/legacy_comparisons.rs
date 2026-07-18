//! Build-163 integer comparison-value selection.
//!
//! The 2.3.3 optimizer materializes 0/1 relations through PowerPC carry chains;
//! the 2.4.x line replaced these with bitwise/count-leading-zero idioms. Keeping
//! the older family here prevents the general comparison driver from becoming a
//! generation fork and gives operand-shape extensions one focused home.

mod narrow;
mod scratch;

use crate::analysis::{constant_value, is_zero_literal};
use crate::expressions::load_base_name;
use crate::generator::{Generator, GENERAL_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_versions::IntegerComparisonValueStyle;

impl Generator {
    pub(crate) fn try_emit_legacy_integer_comparison(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        signed: bool,
    ) -> Compilation<bool> {
        if self.behavior.integer_comparison_value_style
            != IntegerComparisonValueStyle::LegacyCarryChain
        {
            return Ok(false);
        }

        if destination == GENERAL_SCRATCH {
            return self.try_emit_legacy_scratch_comparison(operator, left, right, signed);
        }

        if self.try_emit_legacy_signed_byte_zero_comparison(
            operator,
            left,
            right,
            destination,
            signed,
        )? {
            return Ok(true);
        }

        if matches!(operator, BinaryOperator::NotEqual) {
            return self.try_emit_legacy_not_equal(left, right, destination);
        }

        if !matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterEqual
        ) {
            return Ok(false);
        }

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

        let (left_register, right_register, left_computed) =
            if let (Some(left), Some(right)) = (left_leaf, right_leaf) {
                (left, right, false)
            } else if signed && is_zero_literal(right) {
                let zero_preloaded = left_leaf.is_none()
                    && operator == BinaryOperator::LessEqual
                    && matches!(left, Expression::Binary { .. });
                if zero_preloaded {
                    self.load_integer_constant(GENERAL_SCRATCH, 0);
                }
                let left = if let Some(register) = left_leaf {
                    register
                } else {
                    // The inclusive carry-chain needs a second sign temporary, so
                    // the old selector keeps its computed operand out of every
                    // source register consumed by the expression. Thus `a+b >= 0`
                    // lands the sum in r5 and retains r4 for the zero sign. Strict
                    // comparisons have no such pressure and may overwrite a dead
                    // source (`a-b < 0` lands in r4).
                    let mut avoid = vec![destination];
                    if matches!(
                        operator,
                        BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                    ) {
                        // Preserve the companion result/sign slot even when it
                        // is not an operand register (`*p <= 0`: r3/r4 signs,
                        // load in r5).
                        if destination < 12 {
                            avoid.push(destination + 1);
                        }
                    }
                    let register = self.fresh_virtual_general_avoiding(avoid);
                    self.evaluate_general(left, register)?;
                    register
                };
                let left_computed = left_leaf.is_none();
                let right = if matches!(operator, BinaryOperator::Less | BinaryOperator::Greater) {
                    let register = if left_computed {
                        destination
                    } else {
                        self.fresh_virtual_general()
                    };
                    self.load_integer_constant(register, 0);
                    register
                } else {
                    if !zero_preloaded {
                        self.load_integer_constant(GENERAL_SCRATCH, 0);
                    }
                    GENERAL_SCRATCH
                };
                (left, right, left_computed)
            } else if signed
                && matches!(
                    operator,
                    BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                )
                && self.is_simple_word_load(left)
                && self.is_simple_word_load(right)
            {
                let mut avoid = vec![destination];
                if destination < 12 {
                    avoid.push(destination + 1);
                }
                for operand in [left, right] {
                    if let Some(register) = load_base_name(operand)
                        .and_then(|name| self.lookup_general(name))
                        .filter(|register| !avoid.contains(register))
                    {
                        avoid.push(register);
                    }
                }
                let left_register = self.fresh_virtual_general_avoiding(avoid);
                if operator == BinaryOperator::LessEqual {
                    self.evaluate_general(right, GENERAL_SCRATCH)?;
                    self.evaluate_general(left, left_register)?;
                } else {
                    self.evaluate_general(left, left_register)?;
                    self.evaluate_general(right, GENERAL_SCRATCH)?;
                }
                (left_register, GENERAL_SCRATCH, true)
            } else if signed && matches!(operator, BinaryOperator::Less | BinaryOperator::Greater) {
                match (left_leaf, right_leaf) {
                    (Some(left), None)
                        if constant_value(right)
                            .is_some_and(|constant| i16::try_from(constant).is_ok()) =>
                    {
                        let right_register = self.fresh_virtual_general();
                        self.load_integer_constant(right_register, constant_value(right).unwrap());
                        (left, right_register, false)
                    }
                    (Some(left), None) if left != destination => {
                        self.evaluate_general(right, destination)?;
                        (left, destination, false)
                    }
                    (None, Some(right)) if right != destination => {
                        self.evaluate_general(left, destination)?;
                        (destination, right, true)
                    }
                    (None, None)
                        if self.is_simple_word_load(left) && self.is_simple_word_load(right) =>
                    {
                        // Load the left value while both address bases are live,
                        // then let the right value reuse the result register. The
                        // first value must avoid both bases (`*p OP *q`: r5/r3).
                        let mut avoid = vec![destination];
                        for operand in [left, right] {
                            if let Some(register) = load_base_name(operand)
                                .and_then(|name| self.lookup_general(name))
                                .filter(|register| !avoid.contains(register))
                            {
                                avoid.push(register);
                            }
                        }
                        let left_register = self.fresh_virtual_general_avoiding(avoid);
                        self.evaluate_general(left, left_register)?;
                        self.evaluate_general(right, destination)?;
                        (left_register, destination, true)
                    }
                    _ => return Ok(false),
                }
            } else {
                return Ok(false);
            };

        if signed {
            match operator {
                BinaryOperator::Less | BinaryOperator::Greater => {
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
                            d: destination,
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
                            d: destination,
                            a: GENERAL_SCRATCH,
                        });
                    self.output
                        .instructions
                        .push(Instruction::ClearLeftImmediate {
                            a: destination,
                            s: destination,
                            clear: 31,
                        });
                }
                BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => {
                    let (high, low) = if operator == BinaryOperator::LessEqual {
                        (right_register, left_register)
                    } else {
                        (left_register, right_register)
                    };
                    let sign_high = if left_computed {
                        destination
                    } else {
                        self.fresh_virtual_general()
                    };
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
                        d: destination,
                        a: sign_high,
                        b: sign_low,
                    });
                }
                _ => unreachable!(),
            }
        } else {
            match operator {
                BinaryOperator::Less | BinaryOperator::Greater => {
                    let (first, second) = if operator == BinaryOperator::Less {
                        (right_register, left_register)
                    } else {
                        (left_register, right_register)
                    };
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromCarrying {
                            d: destination,
                            a: first,
                            b: second,
                        });
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromExtended {
                            d: GENERAL_SCRATCH,
                            a: GENERAL_SCRATCH,
                            b: GENERAL_SCRATCH,
                        });
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: GENERAL_SCRATCH,
                    });
                }
                BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => {
                    let (low, high) = if operator == BinaryOperator::LessEqual {
                        (left_register, right_register)
                    } else {
                        (right_register, left_register)
                    };
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromCarrying {
                            d: GENERAL_SCRATCH,
                            a: low,
                            b: high,
                        });
                    self.load_integer_constant(GENERAL_SCRATCH, -1);
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromZeroExtended {
                            d: destination,
                            a: GENERAL_SCRATCH,
                        });
                }
                _ => unreachable!(),
            }
        }
        Ok(true)
    }

    fn try_emit_legacy_not_equal(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if !is_zero_literal(right) {
            if let (Ok((left_register, 32, _)), Some(constant)) =
                (self.leaf_info(left), constant_value(right))
            {
                if let Ok(immediate) = i16::try_from(constant) {
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromImmediate {
                            d: destination,
                            a: left_register,
                            immediate,
                        });
                } else {
                    self.load_integer_constant(GENERAL_SCRATCH, constant);
                    self.output.instructions.push(Instruction::SubtractFrom {
                        d: destination,
                        a: left_register,
                        b: GENERAL_SCRATCH,
                    });
                }
                self.emit_legacy_not_equal_tail(destination, destination);
                return Ok(true);
            }
        }

        if is_zero_literal(right) {
            if let Ok((source, width, _)) = self.leaf_info(left) {
                if width != 32 {
                    return Ok(false);
                }
                self.output.instructions.push(Instruction::Negate {
                    d: destination,
                    a: source,
                });
            } else {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                self.output.instructions.push(Instruction::Negate {
                    d: destination,
                    a: GENERAL_SCRATCH,
                });
            }
        } else {
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
            let (left_register, right_register) = match (left_leaf, right_leaf) {
                (Some(left), Some(right)) => (left, right),
                (Some(left), None) if !matches!(right, Expression::IntegerLiteral(_)) => {
                    self.evaluate_general(right, GENERAL_SCRATCH)?;
                    (left, GENERAL_SCRATCH)
                }
                (None, Some(right)) if !matches!(left, Expression::IntegerLiteral(_)) => {
                    self.evaluate_general(left, GENERAL_SCRATCH)?;
                    (GENERAL_SCRATCH, right)
                }
                (None, None)
                    if self.is_simple_word_load(left) && self.is_simple_word_load(right) =>
                {
                    let left_base = load_base_name(left).and_then(|name| self.lookup_general(name));
                    let right_base =
                        load_base_name(right).and_then(|name| self.lookup_general(name));
                    let left_register = if left_base == right_base {
                        self.fresh_virtual_general_avoiding(
                            left_base.into_iter().chain([destination]).collect(),
                        )
                    } else {
                        destination
                    };
                    self.evaluate_general(left, left_register)?;
                    self.evaluate_general(right, GENERAL_SCRATCH)?;
                    (left_register, GENERAL_SCRATCH)
                }
                _ => return Ok(false),
            };
            self.output.instructions.push(Instruction::SubtractFrom {
                d: destination,
                a: left_register,
                b: right_register,
            });
        }
        self.emit_legacy_not_equal_tail(destination, destination);
        Ok(true)
    }

    pub(super) fn emit_legacy_not_equal_tail(&mut self, value: u8, destination: u8) {
        self.output
            .instructions
            .push(Instruction::AddImmediateCarrying {
                d: GENERAL_SCRATCH,
                a: value,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromExtended {
                d: destination,
                a: GENERAL_SCRATCH,
                b: value,
            });
    }
}
