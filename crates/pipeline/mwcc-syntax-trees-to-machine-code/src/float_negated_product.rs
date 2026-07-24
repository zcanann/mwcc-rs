//! A directly-negated memory value multiplied by another memory value.
//!
//! This is not the fused-negative multiply/add family. MWCC loads both values,
//! negates the source-side value in its allocated home, then emits an ordinary
//! multiply into `f0`. Keeping the negated lifetime virtual preserves a live
//! `f1` when one exists and still colors to `f1` in the measured leaf.

use crate::float_abs_select::abs_select_value;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, MachineFunction, RelocationTarget};
use mwcc_syntax_trees::{Expression, UnaryOperator};

impl Generator {
    /// Lower `[ - ]member * ABS(other_member)` as a paired select/product.
    /// This ends in an ordinary multiply; it is not the fused-negative
    /// multiply/add family.
    pub(crate) fn try_emit_located_abs_product(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        let (source, negate) = match left {
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } if self.is_float_located(operand) => (operand.as_ref(), true),
            expression if self.is_float_located(expression) => (expression, false),
            _ => return Ok(false),
        };
        if destination != FLOAT_SCRATCH || abs_select_value(right).is_none() {
            return Ok(false);
        }

        let later_product = self
            .output
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::StoreFloatSingle { .. }));
        let select_start = self.output.instructions.len();
        let multiplier = self.fresh_virtual_float_preferring(1);
        self.evaluate_float(right, multiplier)?;
        if later_product
            && schedule_later_absolute_value_load(&mut self.output, select_start, multiplier)
        {
            self.output.pre_scheduled = true;
        }
        self.emit_located_operand(source, FLOAT_SCRATCH)?;
        if negate {
            self.output.instructions.push(Instruction::FloatNegate {
                d: FLOAT_SCRATCH,
                b: FLOAT_SCRATCH,
            });
        }
        self.output.instructions.push(if double {
            Instruction::FloatMultiplyDouble {
                d: destination,
                a: FLOAT_SCRATCH,
                c: multiplier,
            }
        } else {
            Instruction::FloatMultiplySingle {
                d: destination,
                a: FLOAT_SCRATCH,
                c: multiplier,
            }
        });
        Ok(true)
    }

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
        if destination != FLOAT_SCRATCH || !self.is_float_located(negated) {
            return Ok(false);
        }

        if !self.is_float_located(right) {
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

/// In a paired update, MWCC begins the first absolute-value select with its
/// pooled zero, but begins the later select with the member value. Move that
/// later member load ahead of the independent pool load and keep its relocation
/// attached to the literal instruction.
fn schedule_later_absolute_value_load(
    output: &mut MachineFunction,
    start: usize,
    multiplier: u8,
) -> bool {
    if output.instructions.len().saturating_sub(start) < 2 {
        return false;
    }
    for index in start..output.instructions.len() - 1 {
        if !matches!(
            output.instructions[index],
            Instruction::LoadFloatSingle { d: 0, a: 0, .. }
        ) || !matches!(
            output.instructions[index + 1],
            Instruction::LoadFloatSingle { d, a, .. } if d == multiplier && a != 0
        ) {
            continue;
        }
        let relocation = output.relocations.iter_mut().find(|relocation| {
            relocation.instruction_index == index
                && matches!(
                    relocation.target,
                    RelocationTarget::Constant(_) | RelocationTarget::ConstantWithAddend(_, _)
                )
        });
        let Some(relocation) = relocation else {
            continue;
        };
        output.instructions.swap(index, index + 1);
        relocation.instruction_index = index + 1;
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    #[test]
    fn places_a_later_absolute_member_before_its_zero_literal() {
        const MULTIPLIER: u8 = 40;
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: MULTIPLIER,
                a: 3,
                offset: 128,
            },
        ];
        output.relocations.push(Relocation {
            instruction_index: 0,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(0),
        });

        assert!(schedule_later_absolute_value_load(
            &mut output,
            0,
            MULTIPLIER
        ));
        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::LoadFloatSingle {
                    d: MULTIPLIER,
                    a: 3,
                    ..
                },
                Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            ]
        ));
        assert_eq!(output.relocations[0].instruction_index, 1);
    }
}
