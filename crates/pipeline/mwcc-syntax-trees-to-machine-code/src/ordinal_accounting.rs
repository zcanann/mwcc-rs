//! Version-specific anonymous-symbol accounting after instruction selection.
//!
//! GC 4.1 exposes optimizer bookkeeping nodes through otherwise-unused `@N`
//! ordinals. They do not change instructions, so keep their structural matching
//! out of body lowering and apply the measured cost to the function's trailing
//! ordinal block here.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, Statement, Type, UnaryOperator,
};
use mwcc_versions::FunctionOrdinalAccountingStyle;

pub(crate) fn apply(
    function: &Function,
    output: &mut MachineFunction,
    style: FunctionOrdinalAccountingStyle,
) {
    let hidden = match style {
        FunctionOrdinalAccountingStyle::Mainline => 0,
        FunctionOrdinalAccountingStyle::Gc41 => gc41_hidden_labels(function, false),
        FunctionOrdinalAccountingStyle::Gc41Ipa => gc41_hidden_labels(function, true),
    };
    output.post_constant_label_bump += hidden;
}

pub(crate) fn apply_unit(
    functions: &[Function],
    machine_functions: &mut [MachineFunction],
    style: FunctionOrdinalAccountingStyle,
) {
    if style != FunctionOrdinalAccountingStyle::Gc41Ipa || machine_functions.is_empty() {
        return;
    }

    let mut saw_float_guard_pool = false;
    let mut unit_front_bump = 0u32;
    for function in functions {
        let Some(machine) = machine_functions
            .iter_mut()
            .find(|machine| machine.name == function.name)
        else {
            continue;
        };
        let has_float_guard_pool = function
            .guards
            .iter()
            .any(|guard| is_float_comparison(&guard.condition))
            && !machine.constants.is_empty();
        if has_float_guard_pool {
            if saw_float_guard_pool {
                // Later pool-bearing float guards are analyzed before pool
                // allocation (+7 at unit front), while three of their four
                // local guard labels coalesce into that unit analysis block.
                unit_front_bump += 7;
                machine.anonymous_label_bump = machine.anonymous_label_bump.saturating_sub(3);
            }
            saw_float_guard_pool = true;
        }
        if function
            .guards
            .iter()
            .any(|guard| is_negated_call_short_circuit(&guard.condition))
        {
            unit_front_bump += 16;
        }
    }
    machine_functions[0].anonymous_label_bump += unit_front_bump;
}

fn gc41_hidden_labels(function: &Function, ipa_file: bool) -> u32 {
    if let [Statement::Store { value, .. }] = function.statements.as_slice() {
        return if matches!(value, Expression::Variable(_)) {
            6
        } else {
            5
        };
    }

    if function.guards.len() == 1 && function.return_expression.is_some() {
        if is_float_comparison(&function.guards[0].condition) {
            // Under file IPA, these trailing nodes join the unit-wide pool-front
            // block instead of remaining after this function's constant.
            return if ipa_file { 0 } else { 4 };
        }
        return 7 + u32::from(ipa_file);
    }

    if let Some(expression) = &function.return_expression {
        if is_float_comparison(expression) {
            return if ipa_file { 0 } else { 3 };
        }
        if is_comparison(expression) {
            return 5;
        }
    }
    0
}

fn is_comparison(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator, .. }
        if crate::analysis::is_comparison(*operator))
}

fn is_float_comparison(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator, left, right }
        if crate::analysis::is_comparison(*operator)
            && (is_float_value(left) || is_float_value(right)))
}

fn is_float_value(expression: &Expression) -> bool {
    match expression {
        Expression::FloatLiteral(_) => true,
        Expression::Cast { target_type, .. } => {
            matches!(target_type, Type::Float | Type::Double)
        }
        _ => false,
    }
}

fn is_negated_call_short_circuit(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
        left,
        right,
    } = expression
    else {
        return false;
    };
    is_negated_call(left) && is_negated_call(right)
}

fn is_negated_call(expression: &Expression) -> bool {
    matches!(expression, Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } if matches!(operand.as_ref(), Expression::Call { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, GuardedReturn};

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "probe".to_string(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
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
        }
    }

    #[test]
    fn gc41_integer_guard_cost_is_ipa_sensitive() {
        let mut function = function();
        function.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::IntegerLiteral(256)),
            },
            value: Expression::IntegerLiteral(1),
        });
        function.return_expression = Some(Expression::IntegerLiteral(0));
        assert_eq!(gc41_hidden_labels(&function, false), 7);
        assert_eq!(gc41_hidden_labels(&function, true), 8);
    }

    #[test]
    fn gc41_ipa_moves_float_guard_trailing_labels_out_of_function() {
        let mut function = function();
        function.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::FloatLiteral(0.5)),
            },
            value: Expression::IntegerLiteral(1),
        });
        function.return_expression = Some(Expression::IntegerLiteral(0));
        assert_eq!(gc41_hidden_labels(&function, false), 4);
        assert_eq!(gc41_hidden_labels(&function, true), 0);
    }

    #[test]
    fn gc41_ipa_accounts_later_float_pool_and_short_circuit_at_unit_front() {
        let mut ground = function();
        ground.name = "ground".to_string();
        ground.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: Box::new(Expression::Variable("value".to_string())),
                right: Box::new(Expression::FloatLiteral(0.5)),
            },
            value: Expression::IntegerLiteral(1),
        });
        ground.return_expression = Some(Expression::IntegerLiteral(0));

        let mut roof = ground.clone();
        roof.name = "roof".to_string();

        let mut wall = function();
        wall.name = "wall".to_string();
        let call = |name: &str| Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(Expression::Call {
                name: name.to_string(),
                arguments: Vec::new(),
            }),
        };
        wall.guards.push(GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(call("ground")),
                right: Box::new(call("roof")),
            },
            value: Expression::IntegerLiteral(1),
        });
        wall.return_expression = Some(Expression::IntegerLiteral(0));

        let mut machines = vec![
            MachineFunction::new("ground"),
            MachineFunction::new("roof"),
            MachineFunction::new("wall"),
        ];
        let pool = |bits| mwcc_machine_code::PoolConstant {
            bits,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
        };
        machines[0].constants.push(pool(0x3f00_0000));
        machines[0].anonymous_label_bump = 4;
        machines[1].constants.push(pool(0xbf4c_cccd));
        machines[1].anonymous_label_bump = 4;

        apply_unit(
            &[ground, roof, wall],
            &mut machines,
            FunctionOrdinalAccountingStyle::Gc41Ipa,
        );
        assert_eq!(machines[0].anonymous_label_bump, 27);
        assert_eq!(machines[1].anonymous_label_bump, 1);
    }
}
