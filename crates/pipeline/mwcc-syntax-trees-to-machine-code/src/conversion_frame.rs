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
use std::collections::HashSet;

impl Generator {
    /// Count prototype-directed integer call arguments that need floating
    /// conversion images. Existing cast/store/arithmetic paths retain their
    /// established frame ownership.
    pub(crate) fn count_integer_to_float_conversions(&self, function: &Function) -> usize {
        fn needs_conversion(
            generator: &Generator,
            declared_float_values: &HashSet<&str>,
            expression: &Expression,
        ) -> bool {
            !(matches!(
                expression,
                Expression::Variable(name) if declared_float_values.contains(name.as_str())
            ) || generator.is_float_value(expression)
                || generator.is_float_operand(expression))
                && constant_value(expression).is_none()
        }

        fn expression_count(
            generator: &Generator,
            declared_float_values: &HashSet<&str>,
            expression: &Expression,
        ) -> usize {
            match expression {
                Expression::Assign { target, value } => {
                    expression_count(generator, declared_float_values, target)
                        + expression_count(generator, declared_float_values, value)
                }
                Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
                    expression_count(generator, declared_float_values, left)
                        + expression_count(generator, declared_float_values, right)
                }
                Expression::Cast {
                    target_type,
                    operand,
                } => {
                    usize::from(
                        matches!(target_type, Type::Float | Type::Double)
                            && needs_conversion(generator, declared_float_values, operand),
                    ) + expression_count(generator, declared_float_values, operand)
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
                            ) && needs_conversion(
                                generator,
                                declared_float_values,
                                argument,
                            )
                        })
                        .count();
                    contextual
                        + arguments
                            .iter()
                            .map(|argument| {
                                expression_count(generator, declared_float_values, argument)
                            })
                            .sum::<usize>()
                }
                Expression::CallThrough { target, arguments } => {
                    expression_count(generator, declared_float_values, target)
                        + arguments
                            .iter()
                            .map(|argument| {
                                expression_count(generator, declared_float_values, argument)
                            })
                            .sum::<usize>()
                }
                Expression::VirtualCall {
                    object, arguments, ..
                } => {
                    expression_count(generator, declared_float_values, object)
                        + arguments
                            .iter()
                            .map(|argument| {
                                expression_count(generator, declared_float_values, argument)
                            })
                            .sum::<usize>()
                }
                Expression::ConstructedNew {
                    allocation,
                    arguments,
                    ..
                } => {
                    expression_count(generator, declared_float_values, allocation)
                        + arguments
                            .iter()
                            .map(|argument| {
                                expression_count(generator, declared_float_values, argument)
                            })
                            .sum::<usize>()
                }
                Expression::Unary { operand, .. }
                | Expression::IndexedUpdateValue { value: operand }
                | Expression::Dereference { pointer: operand }
                | Expression::AddressOf { operand }
                | Expression::PostStep {
                    target: operand, ..
                } => expression_count(generator, declared_float_values, operand),
                Expression::Conditional {
                    condition,
                    when_true,
                    when_false,
                    ..
                } => {
                    expression_count(generator, declared_float_values, condition)
                        + expression_count(generator, declared_float_values, when_true)
                        + expression_count(generator, declared_float_values, when_false)
                }
                Expression::BitFieldRead {
                    extracted, storage, ..
                }
                | Expression::Index {
                    base: extracted,
                    index: storage,
                } => {
                    expression_count(generator, declared_float_values, extracted)
                        + expression_count(generator, declared_float_values, storage)
                }
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                    expression_count(generator, declared_float_values, base)
                }
                Expression::AggregateLiteral(elements) => elements
                    .iter()
                    .map(|element| {
                        expression_count(generator, declared_float_values, element)
                    })
                    .sum(),
                Expression::IntegerLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::Variable(_)
                | Expression::CompoundLiteral { .. } => 0,
            }
        }

        fn arm_count(
            generator: &Generator,
            declared_float_values: &HashSet<&str>,
            arm: &ArmBody,
        ) -> usize {
            match arm {
                ArmBody::Return(expression) => {
                    expression_count(generator, declared_float_values, expression)
                }
                ArmBody::Statements(statements) => {
                    statement_count(generator, declared_float_values, statements, None)
                }
            }
        }

        fn statement_count(
            generator: &Generator,
            declared_float_values: &HashSet<&str>,
            statements: &[Statement],
            return_type: Option<Type>,
        ) -> usize {
            statements
                .iter()
                .map(|statement| match statement {
                    Statement::Store { target, value } => {
                        expression_count(generator, declared_float_values, target)
                            + expression_count(generator, declared_float_values, value)
                    }
                    Statement::Assign { value, .. } | Statement::Expression(value) => {
                        expression_count(generator, declared_float_values, value)
                    }
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    } => {
                        expression_count(generator, declared_float_values, condition)
                            + statement_count(
                                generator,
                                declared_float_values,
                                then_body,
                                return_type,
                            )
                            + statement_count(
                                generator,
                                declared_float_values,
                                else_body,
                                return_type,
                            )
                    }
                    Statement::Return(value) => value
                        .as_ref()
                        .map_or(0, |value| {
                            expression_count(generator, declared_float_values, value)
                        }),
                    Statement::Switch {
                        scrutinee,
                        arms,
                        default,
                    } => {
                        expression_count(generator, declared_float_values, scrutinee)
                            + arms
                                .iter()
                                .map(|arm| {
                                    arm_count(generator, declared_float_values, &arm.body)
                                })
                                .sum::<usize>()
                            + default.as_ref().map_or(0, |arm| {
                                arm_count(generator, declared_float_values, arm)
                            })
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
                            .map_or(0, |value| {
                                expression_count(generator, declared_float_values, value)
                            })
                            + condition
                                .as_ref()
                                .map_or(0, |value| {
                                    expression_count(generator, declared_float_values, value)
                                })
                            + step
                                .as_ref()
                                .map_or(0, |value| {
                                    expression_count(generator, declared_float_values, value)
                                })
                            + statement_count(
                                generator,
                                declared_float_values,
                                body,
                                return_type,
                            )
                    }
                    Statement::Break
                    | Statement::Continue
                    | Statement::Goto(_)
                    | Statement::Label(_)
                    | Statement::InlineAsm(_) => 0,
                })
                .sum()
        }

        let declared_float_values = declared_float_value_names(function);
        statement_count(
            self,
            &declared_float_values,
            &function.statements,
            Some(function.return_type),
        )
            + function
                .return_expression
                .as_ref()
                .map_or(0, |value| {
                    expression_count(self, &declared_float_values, value)
                })
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

fn declared_float_value_names(function: &Function) -> HashSet<&str> {
    function
        .parameters
        .iter()
        .filter(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double))
        .map(|parameter| parameter.name.as_str())
        .chain(
            function
                .locals
                .iter()
                .filter(|local| matches!(local.declared_type, Type::Float | Type::Double))
                .map(|local| local.name.as_str()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn recognizes_unallocated_float_parameters_and_locals() {
        let function = Function {
            return_type: Type::Void,
            name: "convert".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Double,
                    name: "input".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "count".into(),
                },
            ],
            locals: vec![
                local("saved", Type::Float),
                local("index", Type::UnsignedInt),
            ],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let names = declared_float_value_names(&function);
        assert_eq!(names, HashSet::from(["input", "saved"]));
    }
}
