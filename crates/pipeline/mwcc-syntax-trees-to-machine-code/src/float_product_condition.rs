//! Floating product placement immediately consumed by a comparison.
//!
//! MWCC schedules the product's memory dependency before the independent pool
//! literal, then performs the multiply. A virtual result may coalesce with a
//! dead factor, but remains separate when structured control flow still needs
//! that parameter afterward.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    pub(crate) fn try_place_float_product_literal_condition(
        &mut self,
        product: &Expression,
        literal: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        let Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } = product
        else {
            return Ok(None);
        };
        let (loaded, leaf, loaded_is_left) = if self.is_float_located(left)
            && self.is_float_leaf(right)
        {
            (left.as_ref(), right.as_ref(), true)
        } else if self.is_float_leaf(left) && self.is_float_located(right) {
            (right.as_ref(), left.as_ref(), false)
        } else {
            return Ok(None);
        };

        let loaded_home = self.fresh_virtual_float_preferring(2);
        let loaded_register = self.place_condition_float_load(loaded, loaded_home)?;
        self.load_float_literal_into(0, literal, double)?;
        let leaf_register = self.float_register_of_leaf(leaf)?;
        let product_register = self.fresh_virtual_float_preferring(leaf_register);
        let (a, c) = if loaded_is_left {
            (loaded_register, leaf_register)
        } else {
            (leaf_register, loaded_register)
        };
        self.output.instructions.push(if double {
            Instruction::FloatMultiplyDouble {
                d: product_register,
                a,
                c,
            }
        } else {
            Instruction::FloatMultiplySingle {
                d: product_register,
                a,
                c,
            }
        });
        Ok(Some((product_register, 0)))
    }
}
