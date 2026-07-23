//! Comparison of two source-level floating absolute-value selects.
//!
//! MWCC evaluates the memory-backed ABS first, retaining both its raw value and
//! selected result, then evaluates the register-backed ABS into `f0`. This is a
//! small dependency schedule shared by friction/clamp code and kept separate
//! from ordinary comparison placement.

use crate::float_abs_select::abs_select_value;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_syntax_trees::Expression;

impl Generator {
    pub(crate) fn try_place_float_abs_pair_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(left_value) = abs_select_value(left) else {
            return Ok(None);
        };
        let Some(right_value) = abs_select_value(right) else {
            return Ok(None);
        };
        if !self.is_float_leaf(left_value) || !self.is_float_located(right_value) {
            return Ok(None);
        }

        let right_source_home = self.fresh_virtual_float_preferring(3);
        let right_source =
            self.place_condition_float_load(right_value, right_source_home)?;
        let right_result = self.fresh_virtual_float_preferring(2);
        self.emit_float_abs_select(right_source, right_result, double)?;

        let left_source = self.float_register_of_leaf(left_value)?;
        self.emit_float_abs_select(left_source, FLOAT_SCRATCH, double)?;
        Ok(Some((FLOAT_SCRATCH, right_result)))
    }
}
