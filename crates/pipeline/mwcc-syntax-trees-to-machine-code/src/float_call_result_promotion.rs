//! Integer call-result promotion beside a call-surviving float value.
//!
//! The structured body owns the saved FPR live range and conversion frame.
//! This expression owner marshals the integer call, converts r3 through the
//! call-result magic-bias schedule, and combines it with the preserved value.

use crate::analysis::is_intrinsic_call;
use crate::casts::IntToFloatSchedule;
use crate::generator::Generator;
use crate::operands::{float_combine, Operands};
use mwcc_core::Compilation;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};
use mwcc_target::Eabi;

impl Generator {
    pub(crate) fn try_emit_integer_call_float_arithmetic(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if !matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
        ) {
            return Ok(false);
        }
        let (float_operand, call, call_is_left) = match (left, right) {
            (call @ Expression::Call { .. }, float @ Expression::Variable(_))
                if self.is_float_leaf(float) =>
            {
                (float, call, true)
            }
            (float @ Expression::Variable(_), call @ Expression::Call { .. })
                if self.is_float_leaf(float) =>
            {
                (float, call, false)
            }
            _ => return Ok(false),
        };
        let Expression::Call { name, arguments } = call else {
            unreachable!("the call-result promotion shape was matched")
        };
        if is_intrinsic_call(name)
            || matches!(
                self.call_return_types.get(name),
                Some(Type::Float | Type::Double)
            )
            || !self.float_location_survives_call(float_operand)
        {
            return Ok(false);
        }
        let float_register = self.float_register_of_leaf(float_operand)?;
        // The measured schedule uses f2 for the bias while f0 assembles the
        // conversion image and the result occupies its consumer destination.
        // Leave rarer conflicts to a later allocator-owned variant.
        const BIAS_REGISTER: u8 = 2;
        if destination == BIAS_REGISTER
            || float_register == BIAS_REGISTER
            || destination == float_register
        {
            return Ok(false);
        }

        let signed = self.signedness_of(call)?;
        let source = Eabi::general_result().number;
        self.emit_call(name, arguments, None, false)?;
        self.emit_int_to_float_body(
            source,
            destination,
            double,
            signed,
            BIAS_REGISTER,
            IntToFloatSchedule::CallResult,
        );
        let operands = if call_is_left {
            Operands::ordered(destination, float_register)?
        } else {
            Operands::ordered(float_register, destination)?
        };
        self.output
            .instructions
            .push(float_combine(operator, destination, operands, double)?);
        Ok(true)
    }
}
