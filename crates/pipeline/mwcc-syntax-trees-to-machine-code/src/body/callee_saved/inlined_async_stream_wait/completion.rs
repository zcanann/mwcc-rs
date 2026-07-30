//! Terminal-state policies for synchronous wrappers around async operations.
//!
//! Some wrappers return a member published by the callback. Others translate
//! each terminal state into a source constant. Keeping this recognition apart
//! prevents the shared starter/critical-section lowering from depending on one
//! particular result policy.

use super::*;

#[derive(Clone)]
pub(super) enum CompletionPlan {
    Member { result_offset: i16 },
    Constants { cases: Vec<(i16, i16)> },
    Mixed {
        result_offset: i16,
        cases: Vec<(i16, i16)>,
    },
}

fn equality_constant(expression: &Expression, variable_name: &str) -> Option<i16> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let value = if variable(left, variable_name) {
        constant_value(right)?
    } else if variable(right, variable_name) {
        constant_value(left)?
    } else {
        return None;
    };
    i16::try_from(value).ok()
}

pub(super) fn constant_terminal_cases(
    statement: &Statement,
    state_name: &str,
) -> Option<(String, Vec<(i16, i16)>)> {
    fn collect(
        statement: &Statement,
        state_name: &str,
        return_name: &mut Option<String>,
        cases: &mut Vec<(i16, i16)>,
    ) -> bool {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = statement
        else {
            return false;
        };
        let Some(state) = equality_constant(condition, state_name) else {
            return false;
        };
        let [Statement::Assign { name, value }, Statement::Break] = then_body.as_slice() else {
            return false;
        };
        let Some(value) = constant_value(value).and_then(|value| i16::try_from(value).ok()) else {
            return false;
        };
        if return_name.as_ref().is_some_and(|found| found != name) {
            return false;
        }
        return_name.get_or_insert_with(|| name.clone());
        cases.push((state, value));
        match else_body.as_slice() {
            [] => true,
            [next] => collect(next, state_name, return_name, cases),
            _ => false,
        }
    }

    let mut return_name = None;
    let mut cases = Vec::new();
    if !collect(statement, state_name, &mut return_name, &mut cases)
        || cases.iter().map(|(state, _)| *state).collect::<Vec<_>>() != [0, -1, 10]
    {
        return None;
    }
    Some((return_name?, cases))
}

pub(super) fn mixed_terminal_cases(
    statement: &Statement,
    state_name: &str,
    block_name: &str,
) -> Option<(String, i16, Vec<(i16, i16)>)> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if equality_constant(condition, state_name) != Some(0) {
        return None;
    }
    let [Statement::Assign { name, value }, Statement::Break] = then_body.as_slice() else {
        return None;
    };
    let value = match value {
        Expression::Cast { operand, .. } => operand.as_ref(),
        value => value,
    };
    let (result_offset, result_type) = member(value, block_name)?;
    if !matches!(result_type, Type::Int | Type::UnsignedInt) {
        return None;
    }
    let [first_constant] = else_body.as_slice() else {
        return None;
    };

    let mut cases = Vec::new();
    let mut current = first_constant;
    loop {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = current
        else {
            return None;
        };
        let state = equality_constant(condition, state_name)?;
        let [Statement::Assign {
            name: constant_name,
            value,
        }, Statement::Break] = then_body.as_slice()
        else {
            return None;
        };
        if constant_name != name {
            return None;
        }
        cases.push((
            state,
            i16::try_from(constant_value(value)?).ok()?,
        ));
        match else_body.as_slice() {
            [] => break,
            [next] => current = next,
            _ => return None,
        }
    }
    (cases.iter().map(|(state, _)| *state).collect::<Vec<_>>() == [-1, 10])
        .then(|| (name.clone(), result_offset, cases))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal_arm(state: i64, result: i64, next: Vec<Statement>) -> Statement {
        Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Variable("state".into())),
                right: Box::new(Expression::IntegerLiteral(state)),
            },
            then_body: vec![
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(result),
                },
                Statement::Break,
            ],
            else_body: next,
        }
    }

    #[test]
    fn recognizes_ordered_terminal_state_constant_mapping() {
        let statement = terminal_arm(
            0,
            0,
            vec![terminal_arm(-1, -1, vec![terminal_arm(10, -3, Vec::new())])],
        );

        let (return_name, cases) =
            constant_terminal_cases(&statement, "state").expect("terminal mapping");
        assert_eq!(return_name, "result");
        assert_eq!(cases, [(0, 0), (-1, -1), (10, -3)]);
    }

    #[test]
    fn rejects_a_different_terminal_state_order() {
        let statement = terminal_arm(
            -1,
            -1,
            vec![terminal_arm(0, 0, vec![terminal_arm(10, -3, Vec::new())])],
        );

        assert!(constant_terminal_cases(&statement, "state").is_none());
    }

    #[test]
    fn recognizes_a_member_success_with_constant_error_results() {
        let success = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Variable("state".into())),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::Cast {
                        target_type: Type::UnsignedInt,
                        operand: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("block".into())),
                            offset: 32,
                            member_type: Type::UnsignedInt,
                            index_stride: None,
                        }),
                    },
                },
                Statement::Break,
            ],
            else_body: vec![terminal_arm(
                -1,
                -1,
                vec![terminal_arm(10, -3, Vec::new())],
            )],
        };

        let (return_name, offset, cases) =
            mixed_terminal_cases(&success, "state", "block").expect("mixed completion");
        assert_eq!(return_name, "result");
        assert_eq!(offset, 32);
        assert_eq!(cases, [(-1, -1), (10, -3)]);
    }
}
