//! Simultaneous lifetimes for assignment-valued comparison operands.

use super::*;

impl Generator {
    pub(super) fn try_emit_assigned_operand_compare(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !matches!(left, Expression::Assign { .. }) || !matches!(right, Expression::Assign { .. })
        {
            return Ok(None);
        }
        let (Some(left_width @ (8 | 16 | 32)), Some(right_width @ (8 | 16 | 32))) = (
            self.unpromoted_integer_width(left),
            self.unpromoted_integer_width(right),
        ) else {
            return Ok(None);
        };
        let signed = self.usual_integer_binary_signedness(left, right)?;
        let left_signed = self.signedness_of(left)?;
        let right_signed = self.signedness_of(right)?;
        let first = self.with_reserved_inputs(right, |generator| {
            let register = generator.fresh_virtual_general();
            generator.evaluate_general(left, register)?;
            Ok(register)
        })?;
        let reserved = self.reserved.insert(first);
        let result = self.evaluate_general(right, GENERAL_SCRATCH);
        if reserved {
            self.reserved.remove(&first);
        }
        result?;
        // Assignment yields the converted target value. Register locals may
        // retain raw narrow bits, so both values need promotion at the use.
        self.emit_widen(first, first, left_width, left_signed);
        self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, right_width, right_signed);
        self.output.instructions.push(if signed {
            Instruction::CompareWord {
                a: first,
                b: GENERAL_SCRATCH,
            }
        } else {
            Instruction::CompareLogicalWord {
                a: first,
                b: GENERAL_SCRATCH,
            }
        });
        Ok(Some(
            false_branch_bo_bi(operator).expect("comparison operator"),
        ))
    }
}
