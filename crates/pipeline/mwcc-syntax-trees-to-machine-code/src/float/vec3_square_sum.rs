//! Optimized single-precision squared-length return scheduling.
//!
//! MWCC starts the two adjacent low-member products before loading the high
//! member, filling the first multiply's latency window with that load.  This is
//! distinct from the general recursive float evaluator's source-tree order.

use crate::analysis::same_operand;
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_versions::Optimization;

impl Generator {
    /// Claim `return z*z + (x*x + y*y)` for three adjacent float members.
    ///
    /// The caller appends `blr`. Requiring the measured member layout and tree
    /// topology keeps this schedule from changing unrelated product sums whose
    /// source order or common-subexpression behavior differs.
    pub(crate) fn try_float_vec3_square_sum_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Float
            || self.behavior.optimization != Optimization::O4
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(Expression::Binary {
            operator: BinaryOperator::Add,
            left: outer,
            right: inner,
        }) = function.return_expression.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: inner_left,
            right: inner_right,
        } = inner.as_ref()
        else {
            return Ok(false);
        };
        let Some((outer, outer_base, outer_offset)) = float_member_square(outer) else {
            return Ok(false);
        };
        let Some((inner_left, inner_left_base, inner_left_offset)) =
            float_member_square(inner_left)
        else {
            return Ok(false);
        };
        let Some((inner_right, inner_right_base, inner_right_offset)) =
            float_member_square(inner_right)
        else {
            return Ok(false);
        };
        if !same_operand(outer_base, inner_left_base)
            || !same_operand(outer_base, inner_right_base)
            || inner_right_offset != inner_left_offset + 4
            || outer_offset != inner_right_offset + 4
        {
            return Ok(false);
        }

        self.emit_located_operand(inner_left, 1)?;
        self.emit_located_operand(inner_right, 0)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 1, c: 1 });
        self.emit_located_operand(outer, 2)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 0, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 2, a: 2, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 0, a: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatAddSingle { d: 1, a: 2, b: 0 });
        Ok(true)
    }
}

fn float_member_square(expression: &Expression) -> Option<(&Expression, &Expression, u32)> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if !same_operand(left, right) {
        return None;
    }
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(_)) {
        return None;
    }
    Some((left, base, *offset))
}
