//! Stack-image planning for integer-to-floating conversions.
//!
//! MWCC assigns each syntactic conversion its own eight-byte image. Structured
//! frame owners must reserve those images before emitting their prologue;
//! simpler functions may discover them lazily and grow their single frame push.

use crate::analysis::{constant_value, is_comparison};
use crate::generator::{Generator, GENERAL_SCRATCH};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};

impl Generator {
    /// Count prototype-directed integer call arguments that need floating
    /// conversion images. Existing cast/store/arithmetic paths retain their
    /// established frame ownership.
    pub(crate) fn count_integer_to_float_conversions(&self, function: &Function) -> usize {
        fn needs_conversion(generator: &Generator, expression: &Expression) -> bool {
            !(generator.is_float_value(expression) || generator.is_float_operand(expression))
                && constant_value(expression).is_none()
        }

        fn expression_count(generator: &Generator, expression: &Expression) -> usize {
            match expression {
                Expression::Assign { target, value } => {
                    expression_count(generator, target) + expression_count(generator, value)
                }
                Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
                    expression_count(generator, left) + expression_count(generator, right)
                }
                Expression::Cast {
                    target_type,
                    operand,
                } => {
                    usize::from(
                        matches!(target_type, Type::Float | Type::Double)
                            && needs_conversion(generator, operand),
                    ) + expression_count(generator, operand)
                }
                Expression::Call { name, arguments } => {
                    let contextual = arguments
                        .iter()
                        .enumerate()
                        .filter(|(index, argument)| {
                            matches!(
                                generator
                                    .call_parameter_types
                                    .get(name)
                                    .and_then(|types| types.get(*index)),
                                Some(Type::Float | Type::Double)
                            ) && needs_conversion(generator, argument)
                        })
                        .count();
                    contextual
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::CallThrough { target, arguments } => {
                    expression_count(generator, target)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::VirtualCall {
                    object, arguments, ..
                } => {
                    expression_count(generator, object)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::ConstructedNew {
                    allocation,
                    arguments,
                    ..
                } => {
                    expression_count(generator, allocation)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::Unary { operand, .. }
                | Expression::IndexedUpdateValue { value: operand }
                | Expression::Dereference { pointer: operand }
                | Expression::AddressOf { operand }
                | Expression::PostStep {
                    target: operand, ..
                } => expression_count(generator, operand),
                Expression::Conditional {
                    condition,
                    when_true,
                    when_false,
                    ..
                } => {
                    expression_count(generator, condition)
                        + expression_count(generator, when_true)
                        + expression_count(generator, when_false)
                }
                Expression::BitFieldRead {
                    extracted, storage, ..
                }
                | Expression::Index {
                    base: extracted,
                    index: storage,
                } => expression_count(generator, extracted) + expression_count(generator, storage),
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                    expression_count(generator, base)
                }
                Expression::AggregateLiteral(elements) => elements
                    .iter()
                    .map(|element| expression_count(generator, element))
                    .sum(),
                Expression::IntegerLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::Variable(_)
                | Expression::CompoundLiteral { .. } => 0,
            }
        }

        fn arm_count(generator: &Generator, arm: &ArmBody) -> usize {
            match arm {
                ArmBody::Return(expression) => expression_count(generator, expression),
                ArmBody::Statements(statements) => statement_count(generator, statements, None),
            }
        }

        fn statement_count(
            generator: &Generator,
            statements: &[Statement],
            return_type: Option<Type>,
        ) -> usize {
            statements
                .iter()
                .map(|statement| match statement {
                    Statement::Store { target, value } => {
                        expression_count(generator, target) + expression_count(generator, value)
                    }
                    Statement::Assign { value, .. } | Statement::Expression(value) => {
                        expression_count(generator, value)
                    }
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    } => {
                        expression_count(generator, condition)
                            + statement_count(generator, then_body, return_type)
                            + statement_count(generator, else_body, return_type)
                    }
                    Statement::Return(value) => value
                        .as_ref()
                        .map_or(0, |value| expression_count(generator, value)),
                    Statement::Switch {
                        scrutinee,
                        arms,
                        default,
                    } => {
                        expression_count(generator, scrutinee)
                            + arms
                                .iter()
                                .map(|arm| arm_count(generator, &arm.body))
                                .sum::<usize>()
                            + default.as_ref().map_or(0, |arm| arm_count(generator, arm))
                    }
                    Statement::Loop {
                        initializer,
                        condition,
                        step,
                        body,
                        ..
                    } => {
                        initializer
                            .as_ref()
                            .map_or(0, |value| expression_count(generator, value))
                            + condition
                                .as_ref()
                                .map_or(0, |value| expression_count(generator, value))
                            + step
                                .as_ref()
                                .map_or(0, |value| expression_count(generator, value))
                            + statement_count(generator, body, return_type)
                    }
                    Statement::Break
                    | Statement::Continue
                    | Statement::Goto(_)
                    | Statement::Label(_)
                    | Statement::InlineAsm(_) => 0,
                })
                .sum()
        }

        statement_count(self, &function.statements, Some(function.return_type))
            + function
                .return_expression
                .as_ref()
                .map_or(0, |value| expression_count(self, value))
    }

    /// Place an integer expression that is about to be converted to floating
    /// point. Leaf values retain their homes; comparisons and other computed
    /// values are evaluated into an allocator-owned virtual register before
    /// the conversion image is assembled.
    pub(crate) fn materialize_integer_conversion_operand(
        &mut self,
        expression: &Expression,
    ) -> Compilation<u8> {
        if let Ok(register) = self.general_register_of_leaf(expression) {
            return Ok(register);
        }
        let register = if self.behavior.legacy_float_cast_schedule
            && matches!(
                expression,
                Expression::Binary { operator, .. } if is_comparison(*operator)
            )
        {
            // Build 163 keeps a computed boolean in r0 through the signed-bias
            // xor/store. That dependency prevents the 0x4330 high word from
            // being hoisted across and then clobbered by comparison lowering.
            GENERAL_SCRATCH
        } else {
            self.fresh_virtual_general()
        };
        self.evaluate_general(expression, register)?;
        Ok(register)
    }

    pub(crate) fn plan_int_to_float_scratch(&mut self, base: i16, count: usize) -> Compilation<()> {
        let bytes = i16::try_from(count.saturating_mul(8))
            .map_err(|_| Diagnostic::error("int-to-float scratch range is too large"))?;
        self.int_to_float_scratch_next = base;
        self.int_to_float_scratch_end = base
            .checked_add(bytes)
            .ok_or_else(|| Diagnostic::error("int-to-float scratch range is too large"))?;
        Ok(())
    }

    pub(crate) fn claim_int_to_float_scratch(&mut self) -> Compilation<i16> {
        if self.int_to_float_scratch_next == 0 {
            self.int_to_float_scratch_next = 8;
        }
        let offset = self.int_to_float_scratch_next;
        let next = offset
            .checked_add(8)
            .ok_or_else(|| Diagnostic::error("int-to-float scratch range is too large"))?;
        if self.int_to_float_scratch_end != 0 && next > self.int_to_float_scratch_end {
            return Err(Diagnostic::error(
                "int-to-float conversion exceeded its planned scratch range",
            ));
        }
        self.int_to_float_scratch_next = next;

        if self.int_to_float_scratch_end == 0 {
            let required = next.saturating_add(15) & !15;
            if self.frame_size == 0 {
                self.frame_size = required;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -required,
                    });
            } else if required > self.frame_size {
                let old_size = self.frame_size;
                let Some(Instruction::StoreWordWithUpdate { offset, .. }) =
                    self.output.instructions.iter_mut().find(|instruction| {
                        matches!(instruction, Instruction::StoreWordWithUpdate {
                            s: 1,
                            a: 1,
                            offset,
                        } if *offset == -old_size)
                    })
                else {
                    return Err(Diagnostic::error(
                        "a growing int-to-float frame is missing its stack push",
                    ));
                };
                *offset = -required;
                self.frame_size = required;
            }
        }
        Ok(offset)
    }
}
