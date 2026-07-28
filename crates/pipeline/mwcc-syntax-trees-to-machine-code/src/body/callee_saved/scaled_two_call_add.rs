//! A scaled call result retained across a second call.
//!
//! This is the commutative two-call sibling of the direct subtraction owner:
//! MWCC evaluates the heavier, scaled operand first, parks it in r31, then
//! evaluates the remaining call and adds both results.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_callee_saved_scaled_two_call_add(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let Some(Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        }) = function.return_expression.as_ref()
        else {
            return Ok(false);
        };
        let ((scaled_name, scaled_arguments, scale), other) =
            if let Some(scaled) = scaled_call(right) {
                (scaled, left.as_ref())
            } else if let Some(scaled) = scaled_call(left) {
                (scaled, right.as_ref())
            } else {
                return Ok(false);
            };
        let Expression::Call {
            name: other_name,
            arguments: other_arguments,
        } = other
        else {
            return Ok(false);
        };
        if !scaled_arguments.is_empty() || !other_arguments.is_empty() || scale == 0 {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general_preferring(31);
        self.callee_saved = vec![saved];
        self.output
            .instructions
            .extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());

        self.emit_call(scaled_name, scaled_arguments, None, false)?;
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: saved,
                a: Eabi::general_result().number,
                immediate: scale,
            });
        self.emit_call(other_name, other_arguments, None, false)?;
        self.output.instructions.push(Instruction::Add {
            d: Eabi::general_result().number,
            a: Eabi::general_result().number,
            b: saved,
        });
        self.emit_epilogue_and_return();
        self.output.symbol_order = vec![scaled_name.clone(), other_name.clone()];
        Ok(true)
    }
}

fn scaled_call(expression: &Expression) -> Option<(&String, &[Expression], i16)> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let (call, scale) = match (left.as_ref(), right.as_ref()) {
        (call @ Expression::Call { .. }, scale) => (call, constant_value(scale)?),
        (scale, call @ Expression::Call { .. }) => (call, constant_value(scale)?),
        _ => return None,
    };
    let scale = i16::try_from(scale).ok()?;
    let Expression::Call { name, arguments } = call else {
        unreachable!("the scaled operand was matched as a call")
    };
    Some((name, arguments, scale))
}
