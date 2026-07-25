//! Contracted multiply-add scheduling when all three operands are memory loads.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    /// Emit the measured `addend + x * y` memory triplet.
    ///
    /// MWCC fills the independent load lanes in f2, f1, then f0 and contracts
    /// directly into f0. Virtual preferences retain that placement while still
    /// letting the allocator avoid a genuinely live FPR in broader contexts.
    pub(crate) fn try_emit_located_fused_triplet(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if double || operator != BinaryOperator::Add {
            return Ok(false);
        }
        let (addend, x, y) = match right {
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x,
                right: y,
            } => (left, x.as_ref(), y.as_ref()),
            _ => match left {
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: x,
                    right: y,
                } => (right, x.as_ref(), y.as_ref()),
                _ => return Ok(false),
            },
        };
        if !self.is_float_located(addend)
            || !self.is_float_located(x)
            || !self.is_float_located(y)
        {
            return Ok(false);
        }

        let (multiplicand, multiplier, addend_register) = if destination == FLOAT_SCRATCH {
            (
                self.fresh_virtual_float_preferring(2),
                self.fresh_virtual_float_preferring(1),
                destination,
            )
        } else {
            // When this triplet is the left child of a larger expression, keep
            // its result in the requested home and load the second factor there.
            // The first factor gets a separate live lane while f0 remains free
            // for both addends and the sibling subtree.
            if destination >= mwcc_vreg::VIRTUAL_BASE {
                self.register_prefer.insert(
                    u32::from(destination - mwcc_vreg::VIRTUAL_BASE),
                    3,
                );
            }
            (
                self.fresh_virtual_float_preferring(4),
                destination,
                FLOAT_SCRATCH,
            )
        };
        self.emit_located_operand(x, multiplicand)?;
        self.emit_located_operand(y, multiplier)?;
        self.emit_located_operand(addend, addend_register)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: destination,
                a: multiplicand,
                c: multiplier,
                b: addend_register,
            });
        Ok(true)
    }
}
