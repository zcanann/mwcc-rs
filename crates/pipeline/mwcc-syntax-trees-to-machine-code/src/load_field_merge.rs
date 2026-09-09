//! Disjoint fields whose zero bits follow from unsigned storage or a cast.
//!
//! The proof of disjointness is separate from operand placement. In particular,
//! a widening unsigned cast of a signed byte does not prove that its upper bits are zero.

use crate::analysis::{
    constant_value, contains_memory_load, expression_has_side_effect, mask_to_run,
};
use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_emit_load_field_merge(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u32,
    ) -> Compilation<bool> {
        if !matches!(
            self.behavior.optimization,
            Optimization::O2 | Optimization::O3 | Optimization::O4
        ) || expression_has_side_effect(left)
            || expression_has_side_effect(right)
            || !(contains_memory_load(left) || contains_memory_load(right))
            || !self.load_field_reads_are_nonvolatile(left)
            || !self.load_field_reads_are_nonvolatile(right)
        {
            return Ok(false);
        }
        let Some((base, inserted, shift, mask)) = self
            .load_field_operands(left, right)
            .or_else(|| self.load_field_operands(right, left))
        else {
            return Ok(false);
        };
        let Some((begin, end)) = mask_to_run(mask) else {
            return Ok(false);
        };
        let source = self.fresh_virtual_general_preferring(0);
        self.with_reserved_inputs(base, |generator| {
            generator.evaluate_general(inserted, source)
        })?;
        self.evaluate_general(base, destination)?;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: destination,
                s: source,
                shift,
                begin,
                end,
            });
        Ok(true)
    }

    // Qualifiers are not represented in the scalar expression type. Use the
    // frontend's pointer provenance before changing load order, and leave
    // volatile/unknown addresses to their existing expression owner.
    fn load_field_reads_are_nonvolatile(&self, expression: &Expression) -> bool {
        let ordinary_pointer = |pointer: &Expression| {
            matches!(pointer, Expression::Variable(name)
                if self.nonvolatile_pointer_bindings.contains(name))
        };
        match expression {
            Expression::IntegerLiteral(_) => true,
            Expression::Variable(name) => self.locations.contains_key(name),
            Expression::Cast { operand, .. } => self.load_field_reads_are_nonvolatile(operand),
            Expression::Binary { left, right, .. } => {
                self.load_field_reads_are_nonvolatile(left)
                    && self.load_field_reads_are_nonvolatile(right)
            }
            Expression::Index { base, index } => {
                ordinary_pointer(base) && self.load_field_reads_are_nonvolatile(index)
            }
            Expression::Dereference { pointer } => ordinary_pointer(pointer),
            Expression::Member {
                base,
                index_stride: None,
                ..
            } => ordinary_pointer(base),
            _ => false,
        }
    }

    fn load_field_operands<'a>(
        &self,
        base: &'a Expression,
        inserted: &'a Expression,
    ) -> Option<(&'a Expression, &'a Expression, u8, u32)> {
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } = inserted
        else {
            return None;
        };
        let shift = u8::try_from(constant_value(right)?).ok()?;
        if !(1..32).contains(&shift) {
            return None;
        }
        let mask = self.load_field_value_mask(left)? << shift;
        let base_mask = self.load_field_value_mask(base)?;
        (mask != 0 && mask & base_mask == 0).then_some((base, left, shift, mask))
    }

    fn load_field_value_mask(&self, expression: &Expression) -> Option<u32> {
        match expression {
            Expression::Cast {
                target_type,
                operand,
            } => {
                if !matches!(
                    target_type,
                    Type::Int
                        | Type::UnsignedInt
                        | Type::Char
                        | Type::UnsignedChar
                        | Type::Short
                        | Type::UnsignedShort
                ) {
                    return None;
                }
                let width = target_type.width();
                if width > 32 || width == 0 {
                    return None;
                }
                let mask = self.load_field_value_mask(operand)?;
                if width == 32 {
                    Some(mask)
                } else if !self.signed_of(*target_type) {
                    Some(mask & ((1u32 << width) - 1))
                } else {
                    None
                }
            }
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            } => Some(self.load_field_value_mask(left)? | self.load_field_value_mask(right)?),
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            } => Some(self.load_field_value_mask(left)? & constant_value(right)? as u32),
            Expression::Binary {
                operator: BinaryOperator::ShiftLeft,
                left,
                right,
            } => {
                let shift = constant_value(right)?;
                (0..32)
                    .contains(&shift)
                    .then(|| self.load_field_value_mask(left).map(|mask| mask << shift))
                    .flatten()
            }
            Expression::Index { .. }
            | Expression::Dereference { .. }
            | Expression::Member { .. }
            | Expression::Variable(_) => {
                let width = self.unpromoted_integer_width(expression)?;
                if self.signedness_of(expression).ok()? || width == 0 || width > 32 {
                    return None;
                }
                Some(if width == 32 {
                    u32::MAX
                } else {
                    (1u32 << width) - 1
                })
            }
            _ => None,
        }
    }
}
