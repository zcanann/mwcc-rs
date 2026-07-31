//! Retained canonical `sqrtf` expression lowering.
//!
//! The SDK header body is too large and stateful for generic expression
//! substitution: it contains static double constants, a guarded Newton
//! iteration, and a volatile float spill.  This owner validates that complete
//! semantic body before emitting the measured inline transaction at a call
//! site.

use crate::generator::{Generator, ValueClass, FLOAT_SCRATCH};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};

pub(crate) fn is_supported_retained_sqrtf(function: &Function) -> bool {
    if function.name != "sqrtf"
        || function.return_type != Type::Float
        || function.parameters.len() != 1
        || function.parameters[0].parameter_type != Type::Float
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return false;
    }
    let parameter = function.parameters[0].name.as_str();
    if !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == parameter)
        || !has_constant_local(function, "_half", 0.5)
        || !has_constant_local(function, "_three", 3.0)
        || !function.locals.iter().any(|local| {
            local.name == "y"
                && local.declared_type == Type::Float
                && local.is_volatile
                && !local.is_static
                && local.initializer.is_none()
        })
        || !function.locals.iter().any(|local| {
            local.name == "guess"
                && local.declared_type == Type::Double
                && !local.is_volatile
                && !local.is_static
                && local.initializer.is_none()
        })
    {
        return false;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return false;
    };
    if !else_body.is_empty()
        || !matches!(condition,
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            } if variable(left, parameter) && float_literal(right, 0.0))
    {
        return false;
    }
    let [estimate, first, second, third, spill, returned] = then_body.as_slice() else {
        return false;
    };
    matches!(estimate,
        Statement::Assign { name, value }
            if name == "guess" && reciprocal_estimate(value, parameter))
        && [first, second, third].iter().all(|statement| {
            matches!(statement,
                Statement::Assign { name, value }
                    if name == "guess" && newton_step(value, parameter))
        })
        && matches!(spill,
            Statement::Assign { name, value }
                if name == "y" && rounded_product(value, parameter))
        && matches!(returned,
            Statement::Return(Some(Expression::Variable(name))) if name == "y")
}

fn has_constant_local(function: &Function, name: &str, value: f64) -> bool {
    function.locals.iter().any(|local| {
        local.name == name
            && local.declared_type == Type::Double
            && local.is_static
            && local.is_const
            && matches!(local.initializer.as_ref(),
                Some(Expression::Cast {
                    target_type: Type::Double,
                    operand,
                }) if float_literal(operand, value))
    })
}

fn reciprocal_estimate(expression: &Expression, parameter: &str) -> bool {
    matches!(expression,
        Expression::Call { name, arguments }
            if name == "__frsqrte"
                && matches!(arguments.as_slice(), [Expression::Cast {
                    target_type: Type::Double,
                    operand,
                }] if variable(operand, parameter)))
}

fn newton_step(expression: &Expression, parameter: &str) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: half,
        right: guess,
    } = left.as_ref()
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: three,
        right: correction,
    } = right.as_ref()
    else {
        return false;
    };
    variable(half, "_half")
        && variable(guess, "guess")
        && variable(three, "_three")
        && matches!(correction.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: squared,
                right: x,
            } if variable(x, parameter)
                && matches!(squared.as_ref(),
                    Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        left,
                        right,
                    } if variable(left, "guess") && variable(right, "guess")))
}

fn rounded_product(expression: &Expression, parameter: &str) -> bool {
    matches!(expression,
        Expression::Cast {
            target_type: Type::Float,
            operand,
        } if matches!(operand.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left,
                right,
            } if variable(left, parameter) && variable(right, "guess")))
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn float_literal(expression: &Expression, expected: f64) -> bool {
    matches!(expression, Expression::FloatLiteral(value) if *value == expected)
}

impl Generator {
    pub(crate) fn is_retained_sqrtf_call(&self, expression: &Expression) -> bool {
        let Expression::Call { name, arguments } = expression else {
            return false;
        };
        arguments.len() == 1
            && self
                .inline_bodies
                .retained_body(name)
                .is_some_and(is_supported_retained_sqrtf)
    }

    pub(crate) fn try_emit_retained_sqrtf(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if !self.is_retained_sqrtf_call(expression) {
            return Ok(false);
        }
        let Expression::Call { arguments, .. } = expression else {
            unreachable!("retained sqrtf classification established a call")
        };
        let input = self.float_register_of_leaf(&arguments[0])?;
        let spill = self
            .frame_slots
            .get("spC")
            .copied()
            .filter(|slot| {
                slot.offset == 12
                    && slot.class == ValueClass::Float
                    && slot.size == 4
                    && slot.value_type == Type::Float
                    && !slot.is_array
            })
            .ok_or_else(|| {
                Diagnostic::error(
                    "retained sqrtf needs its recovered volatile float spill at r1+12",
                )
            })?;

        self.output.has_float_branch = true;
        self.load_float_constant(FLOAT_SCRATCH, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: input,
                b: FLOAT_SCRATCH,
            });
        let nonpositive = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, nonpositive);

        let guess = self.fresh_virtual_float_preferring(31);
        self.output
            .instructions
            .push(Instruction::FloatReciprocalSqrtEstimate { d: guess, b: input });
        let half_guess = self.fresh_virtual_float_preferring(2);
        for _ in 0..3 {
            self.load_double_constant(FLOAT_SCRATCH, 0.5f64.to_bits());
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble {
                    d: half_guess,
                    a: FLOAT_SCRATCH,
                    c: guess,
                });
            self.load_double_constant(1, 3.0f64.to_bits());
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble {
                    d: FLOAT_SCRATCH,
                    a: guess,
                    c: guess,
                });
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble {
                    d: FLOAT_SCRATCH,
                    a: input,
                    c: FLOAT_SCRATCH,
                });
            self.output
                .instructions
                .push(Instruction::FloatSubtractDouble {
                    d: FLOAT_SCRATCH,
                    a: 1,
                    b: FLOAT_SCRATCH,
                });
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble {
                    d: guess,
                    a: half_guess,
                    c: FLOAT_SCRATCH,
                });
        }
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble {
                d: FLOAT_SCRATCH,
                a: input,
                c: guess,
            });
        self.output.instructions.push(Instruction::RoundToSingle {
            d: FLOAT_SCRATCH,
            b: FLOAT_SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: FLOAT_SCRATCH,
                a: 1,
                offset: spill.offset,
            });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: destination,
            a: 1,
            offset: spill.offset,
        });
        self.emit_branch_to(join);
        self.bind_label(nonpositive);
        self.output.instructions.push(Instruction::FloatMove {
            d: destination,
            b: input,
        });
        self.bind_label(join);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{float_literal, variable};
    use mwcc_syntax_trees::Expression;

    #[test]
    fn leaf_helpers_distinguish_names_and_exact_literals() {
        assert!(variable(&Expression::Variable("x".into()), "x"));
        assert!(!variable(&Expression::Variable("y".into()), "x"));
        assert!(float_literal(&Expression::FloatLiteral(0.5), 0.5));
        assert!(!float_literal(&Expression::FloatLiteral(0.25), 0.5));
    }
}
