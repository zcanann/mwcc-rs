//! Version-specific anonymous-symbol accounting after instruction selection.
//!
//! GC 4.1 exposes optimizer bookkeeping nodes through otherwise-unused `@N`
//! ordinals. They do not change instructions, so keep their structural matching
//! out of body lowering and apply the measured cost to the function's trailing
//! ordinal block here.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
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
            asm_body: None,
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
}
