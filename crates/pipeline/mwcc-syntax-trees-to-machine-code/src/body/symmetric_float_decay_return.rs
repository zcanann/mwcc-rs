//! Floating-point decay toward zero returned from a local result.
//!
//! MWCC aliases the source-level result local onto the incoming result FPR,
//! keeps one pooled zero live across both sign arms, and turns every no-clamp
//! edge into a conditional return. Owning the complete function keeps those
//! cross-branch lifetimes out of the ordinary statement walker.

#[allow(unused_imports)]
use super::*;

struct SymmetricFloatDecayReturn<'a> {
    value: &'a str,
    decrement: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn update_result(
    statement: &Statement,
    result: &str,
    decrement: &str,
    operator: BinaryOperator,
) -> bool {
    matches!(statement,
        Statement::Assign {
            name,
            value: Expression::Binary { operator: actual, left, right },
        } if name == result
            && *actual == operator
            && variable(left, result)
            && variable(right, decrement))
}

fn zero_return_guard(
    statement: &Statement,
    result: &str,
    comparison: BinaryOperator,
) -> bool {
    let Statement::If {
        condition:
            Expression::Binary {
                operator,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    *operator == comparison
        && variable(left, result)
        && is_zero_literal(right)
        && matches!(then_body.as_slice(), [Statement::Return(Some(value))] if is_zero_literal(value))
        && else_body.is_empty()
}

fn classify(function: &Function) -> Option<SymmetricFloatDecayReturn<'_>> {
    if function.return_type != Type::Float
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [value, decrement] = function.parameters.as_slice() else {
        return None;
    };
    if value.parameter_type != Type::Float || decrement.parameter_type != Type::Float {
        return None;
    }
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if result.declared_type != Type::Float
        || result.is_static
        || result.is_volatile
        || result.array_length.is_some()
        || !result
            .initializer
            .as_ref()
            .is_some_and(|initializer| variable(initializer, &value.name))
        || !function
            .return_expression
            .as_ref()
            .is_some_and(|expression| variable(expression, &result.name))
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: positive_value,
                right: positive_zero,
            },
        then_body: positive,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [positive_update, positive_guard] = positive.as_slice() else {
        return None;
    };
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: negative_value,
                right: negative_zero,
            },
        then_body: negative,
        else_body: negative_else,
    }] = else_body.as_slice()
    else {
        return None;
    };
    let [negative_update, negative_guard] = negative.as_slice() else {
        return None;
    };
    if !variable(positive_value, &value.name)
        || !is_zero_literal(positive_zero)
        || !update_result(
            positive_update,
            &result.name,
            &decrement.name,
            BinaryOperator::Subtract,
        )
        || !zero_return_guard(positive_guard, &result.name, BinaryOperator::Less)
        || !variable(negative_value, &value.name)
        || !is_zero_literal(negative_zero)
        || !negative_else.is_empty()
        || !update_result(
            negative_update,
            &result.name,
            &decrement.name,
            BinaryOperator::Add,
        )
        || !zero_return_guard(negative_guard, &result.name, BinaryOperator::Greater)
    {
        return None;
    }
    Some(SymmetricFloatDecayReturn {
        value: &value.name,
        decrement: &decrement.name,
    })
}

impl Generator {
    pub(crate) fn try_symmetric_float_decay_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let value = self.float_register_of(shape.value)?;
        let decrement = self.float_register_of(shape.decrement)?;
        if value != 1 || decrement != 2 {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.load_float_constant(0, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: value, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 8,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: value,
                a: value,
                b: decrement,
            });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: value, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: value, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            });
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: value,
            a: value,
            b: decrement,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: value, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: value, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
