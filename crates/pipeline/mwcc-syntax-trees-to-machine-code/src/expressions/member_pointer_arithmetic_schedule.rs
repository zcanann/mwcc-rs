//! Instruction schedule for arithmetic on a non-power-of-two struct pointer member.
//!
//! A multiply has enough latency that MWCC starts it before the independent
//! member-pointer load. Power-of-two strides use a one-cycle rotate/shift and
//! retain the ordinary pointer-first evaluation order.

#[allow(unused_imports)]
use super::*;

fn non_power_member_pointer_add<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
) -> Option<(&'a Expression, &'a Expression, u32)> {
    if operator != BinaryOperator::Add {
        return None;
    }
    for (pointer, index) in [(left, right), (right, left)] {
        let Some(stride) = super::pointers::pointer_member_stride(pointer) else {
            continue;
        };
        if stride > 1
            && !stride.is_power_of_two()
            && constant_value(index).is_none()
            && leaf_name(index).is_some()
        {
            return Some((pointer, index, stride));
        }
    }
    None
}

impl Generator {
    pub(crate) fn try_emit_non_power_member_pointer_add(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some((pointer_expression, index, stride)) =
            non_power_member_pointer_add(operator, left, right)
        else {
            return Ok(false);
        };
        let index = self.general_register_of_leaf(index)?;
        let stride = i16::try_from(stride)
            .map_err(|_| Diagnostic::error("pointer stride out of range (roadmap)"))?;

        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: GENERAL_SCRATCH,
                a: index,
                immediate: stride,
            });
        let pointer = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        self.evaluate_general(pointer_expression, pointer)?;
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: pointer,
            b: GENERAL_SCRATCH,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(stride: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 44,
            member_type: Type::StructPointer {
                element_size: stride,
            },
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_only_variable_non_power_member_pointer_adds() {
        let pointer = member(12);
        let index = Expression::Variable("index".into());
        assert!(non_power_member_pointer_add(BinaryOperator::Add, &pointer, &index).is_some());
        assert!(non_power_member_pointer_add(BinaryOperator::Add, &index, &pointer).is_some());
        assert!(non_power_member_pointer_add(
            BinaryOperator::Add,
            &member(8),
            &index,
        )
        .is_none());
        assert!(non_power_member_pointer_add(
            BinaryOperator::Subtract,
            &pointer,
            &index,
        )
        .is_none());
        assert!(non_power_member_pointer_add(
            BinaryOperator::Add,
            &pointer,
            &Expression::IntegerLiteral(2),
        )
        .is_none());
    }
}
