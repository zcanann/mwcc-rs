//! Runtime-backed scalar conversions.
//!
//! Some scalar casts are ABI calls rather than machine instructions. Keeping
//! their detection and emission together lets body planning reserve a non-leaf
//! frame before expression selection reaches the hidden call.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Pointee, Type};
use mwcc_target::Eabi;

const FLOAT_TO_UNSIGNED: &str = "__cvt_fp2unsigned";

impl Generator {
    /// Whether evaluating `expression` can invoke the float-to-unsigned helper.
    pub(crate) fn expression_needs_float_to_unsigned_helper(
        &self,
        expression: &Expression,
    ) -> bool {
        match expression {
            Expression::Cast {
                target_type: Type::UnsignedInt,
                operand,
            } => {
                self.is_float_value(operand)
                    || self.expression_needs_float_to_unsigned_helper(operand)
            }
            Expression::Binary { left, right, .. } => {
                self.expression_needs_float_to_unsigned_helper(left)
                    || self.expression_needs_float_to_unsigned_helper(right)
            }
            Expression::Unary { operand, .. }
            | Expression::AddressOf { operand }
            | Expression::IndexedUpdateValue { value: operand }
            | Expression::Dereference { pointer: operand }
            | Expression::PostStep {
                target: operand, ..
            } => self.expression_needs_float_to_unsigned_helper(operand),
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } => {
                self.expression_needs_float_to_unsigned_helper(condition)
                    || self.expression_needs_float_to_unsigned_helper(when_true)
                    || self.expression_needs_float_to_unsigned_helper(when_false)
            }
            Expression::Assign { target, value } => {
                self.expression_needs_float_to_unsigned_helper(target)
                    || self.expression_needs_float_to_unsigned_helper(value)
            }
            Expression::Comma { left, right }
            | Expression::Index {
                base: left,
                index: right,
            } => {
                self.expression_needs_float_to_unsigned_helper(left)
                    || self.expression_needs_float_to_unsigned_helper(right)
            }
            Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                self.expression_needs_float_to_unsigned_helper(base)
            }
            Expression::BitFieldRead {
                extracted, storage, ..
            } => {
                self.expression_needs_float_to_unsigned_helper(extracted)
                    || self.expression_needs_float_to_unsigned_helper(storage)
            }
            Expression::Call { arguments, .. } => arguments
                .iter()
                .any(|argument| self.expression_needs_float_to_unsigned_helper(argument)),
            Expression::CallThrough { target, arguments } => {
                self.expression_needs_float_to_unsigned_helper(target)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_needs_float_to_unsigned_helper(argument))
            }
            Expression::VirtualCall {
                object, arguments, ..
            } => {
                self.expression_needs_float_to_unsigned_helper(object)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_needs_float_to_unsigned_helper(argument))
            }
            Expression::ConstructedNew {
                allocation,
                arguments,
                ..
            } => {
                self.expression_needs_float_to_unsigned_helper(allocation)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_needs_float_to_unsigned_helper(argument))
            }
            Expression::AggregateLiteral(values) => values
                .iter()
                .any(|value| self.expression_needs_float_to_unsigned_helper(value)),
            Expression::IntegerLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::Variable(_)
            | Expression::CompoundLiteral { .. }
            | Expression::Cast { .. } => false,
        }
    }

    /// Whether the return conversion introduces a hidden runtime call even
    /// though no call appears in the syntax tree.
    pub(crate) fn return_needs_float_to_unsigned_helper(&self, function: &Function) -> bool {
        let Some(expression) = function.return_expression.as_ref() else {
            return false;
        };
        (function.return_type == Type::UnsignedInt && self.is_float_value(expression))
            || self.expression_needs_float_to_unsigned_helper(expression)
    }

    /// Convert a floating expression to `unsigned int` through the EABI helper.
    pub(crate) fn emit_float_to_unsigned_integer(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        if !self.try_emit_scaled_float_to_unsigned_argument(operand)? {
            self.evaluate_float(operand, Eabi::float_result().number)?;
        }
        self.emit_call(FLOAT_TO_UNSIGNED, &[], Some(destination), false)
    }

    /// Lower `bias + loaded_value * scale` into the helper's f1 argument.
    ///
    /// MWCC starts the global/member address before loading the two double
    /// constants, then keeps the loaded value in f2 while f1 holds the scale.
    /// This differs from an ordinary float return, where the final destination
    /// can own the addend from the beginning.
    fn try_emit_scaled_float_to_unsigned_argument(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = expression
        else {
            return Ok(false);
        };
        let (bias, product) = match (double_literal(left), double_literal(right)) {
            (Some(bias), None) => (bias, right.as_ref()),
            (None, Some(bias)) => (bias, left.as_ref()),
            _ => return Ok(false),
        };
        let Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } = product
        else {
            return Ok(false);
        };
        let (loaded, scale) = match (double_literal(left), double_literal(right)) {
            (Some(scale), None) => (right.as_ref(), scale),
            (None, Some(scale)) => (left.as_ref(), scale),
            _ => return Ok(false),
        };
        if !self.is_float_located(loaded) {
            return Ok(false);
        }

        const LOADED: u8 = 2;
        const SCRATCH: u8 = 0;
        let argument = Eabi::float_result().number;
        if let Some((global, displacement, element)) = global_member_element(loaded) {
            let address = self.fresh_virtual_general_preferring(3);
            self.emit_address_of(global, address)?;
            self.load_float_literal(argument, scale, true);
            self.output.instructions.push(match element {
                Pointee::Float => Instruction::LoadFloatSingle {
                    d: LOADED,
                    a: address,
                    offset: displacement,
                },
                Pointee::Double => Instruction::LoadFloatDouble {
                    d: LOADED,
                    a: address,
                    offset: displacement,
                },
                _ => unreachable!("the recognizer accepts only floating members"),
            });
        } else {
            self.evaluate_float(loaded, LOADED)?;
            self.load_float_literal(argument, scale, true);
        }
        self.load_float_literal(SCRATCH, bias, true);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble {
                d: LOADED,
                a: LOADED,
                c: argument,
            });
        self.output.instructions.push(Instruction::FloatAddDouble {
            d: argument,
            a: SCRATCH,
            b: LOADED,
        });
        Ok(true)
    }
}

fn double_literal(expression: &Expression) -> Option<f64> {
    match expression {
        Expression::Cast {
            target_type: Type::Double,
            operand,
        } => match operand.as_ref() {
            Expression::FloatLiteral(value) => Some(*value),
            _ => None,
        },
        Expression::FloatLiteral(value) => Some(*value),
        _ => None,
    }
}

fn global_member_element(expression: &Expression) -> Option<(&Expression, i16, Pointee)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::MemberAddress {
        base: global,
        offset,
        element: element @ (Pointee::Float | Pointee::Double),
        ..
    } = base.as_ref()
    else {
        return None;
    };
    if !matches!(global.as_ref(), Expression::Variable(_)) {
        return None;
    }
    let index = crate::analysis::constant_value(index)?;
    let displacement = i64::from(*offset) + index.checked_mul(i64::from(element.size()))?;
    Some((global.as_ref(), i16::try_from(displacement).ok()?, *element))
}
