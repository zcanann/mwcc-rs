//! Reuse of a compared member on a structured if/else false edge.
//!
//! A select such as `result = member == K ? C : member` already has the member
//! in the comparison register. Legacy MWCC copies that value into the result
//! home on the false edge instead of reloading through the object pointer.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct MemberElseReuse<'a> {
    result: &'a str,
    base: &'a str,
}

pub(super) fn member_else_reuse_plan<'a>(
    condition: &'a Expression,
    then_body: &'a [Statement],
    else_body: &'a [Statement],
) -> Option<MemberElseReuse<'a>> {
    let Expression::Binary {
        operator:
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let compared_member = match (left.as_ref(), right.as_ref()) {
        (member @ Expression::Member { .. }, Expression::IntegerLiteral(_))
        | (Expression::IntegerLiteral(_), member @ Expression::Member { .. }) => member,
        _ => return None,
    };
    let Expression::Member { base, .. } = compared_member else {
        unreachable!()
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    let (
        [Statement::Assign {
            name: then_result,
            value: Expression::IntegerLiteral(_),
        }],
        [Statement::Assign {
            name: else_result,
            value: else_value,
        }],
    ) = (then_body, else_body)
    else {
        return None;
    };
    (then_result == else_result && structurally_equal(else_value, compared_member)).then_some(
        MemberElseReuse {
            result: then_result,
            base,
        },
    )
}

/// Whether the false-edge select proves a deferred result can take the
/// compared parameter's saved home. Both edges define the result only after
/// the parameter's final use on that edge.
pub(super) fn function_member_select_reuses_parameter(
    function: &Function,
    result: &str,
    parameter: &str,
) -> bool {
    let mut match_index = None;
    for (index, statement) in function.statements.iter().enumerate() {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = statement
        else {
            continue;
        };
        let Some(plan) = member_else_reuse_plan(condition, then_body, else_body) else {
            continue;
        };
        if plan.result == result && plan.base == parameter {
            if match_index.replace(index).is_some() {
                return false;
            }
        }
    }
    let Some(match_index) = match_index else {
        return false;
    };
    if function
        .statements
        .iter()
        .skip(match_index + 1)
        .any(|statement| statement_reads_name(statement, parameter))
    {
        return false;
    }
    count_statement_assignments(&function.statements, result) == 2
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_reads_name(value, name)
        }
        // Opaque assembly and non-local control flow defeat this local
        // last-use proof; decline parameter-home reuse conservatively.
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => true,
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
                || else_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
        }
        Statement::Return(value) => value
            .as_ref()
            .is_some_and(|value| expression_reads_name(value, name)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms.iter().any(|arm| arm_body_reads_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_reads_name(body, name))
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .iter()
                .chain(condition)
                .chain(step)
                .any(|value| expression_reads_name(value, name))
                || body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name))
        }
    }
}

fn arm_body_reads_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(value) => expression_reads_name(value, name),
        mwcc_syntax_trees::ArmBody::Statements(statements) => statements
            .iter()
            .any(|statement| statement_reads_name(statement, name)),
    }
}

fn count_statement_assignments(statements: &[Statement], name: &str) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Store { target, value } => {
                super::structured_locals::expression_assignment_count(target, name)
                    + super::structured_locals::expression_assignment_count(value, name)
            }
            Statement::Assign {
                name: assigned,
                value,
            } => {
                usize::from(assigned == name)
                    + super::structured_locals::expression_assignment_count(value, name)
            }
            Statement::Expression(value) => {
                super::structured_locals::expression_assignment_count(value, name)
            }
            Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => 0,
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                super::structured_locals::expression_assignment_count(condition, name)
                    + count_statement_assignments(then_body, name)
                    + count_statement_assignments(else_body, name)
            }
            Statement::Return(value) => value.as_ref().map_or(0, |value| {
                super::structured_locals::expression_assignment_count(value, name)
            }),
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                initializer
                    .iter()
                    .chain(condition)
                    .chain(step)
                    .map(|value| super::structured_locals::expression_assignment_count(value, name))
                    .sum::<usize>()
                    + count_statement_assignments(body, name)
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                super::structured_locals::expression_assignment_count(scrutinee, name)
                    + arms
                        .iter()
                        .map(|arm| count_arm_assignments(&arm.body, name))
                        .sum::<usize>()
                    + default
                        .as_ref()
                        .map_or(0, |body| count_arm_assignments(body, name))
            }
        })
        .sum()
}

fn count_arm_assignments(body: &mwcc_syntax_trees::ArmBody, name: &str) -> usize {
    match body {
        mwcc_syntax_trees::ArmBody::Return(value) => {
            super::structured_locals::expression_assignment_count(value, name)
        }
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            count_statement_assignments(statements, name)
        }
    }
}

pub(super) fn compared_register_before_branch(
    instructions: &[Instruction],
    branch: usize,
) -> Option<u8> {
    match instructions.get(branch.checked_sub(1)?)? {
        Instruction::CompareWordImmediate { a, .. }
        | Instruction::CompareLogicalWordImmediate { a, .. } => Some(*a),
        _ => None,
    }
}

impl Generator {
    pub(super) fn emit_member_else_reuse(&mut self, plan: MemberElseReuse<'_>, source: u8) -> bool {
        let Some(location) = self.locations.get(plan.result) else {
            return false;
        };
        if location.class != ValueClass::General || location.register == GENERAL_SCRATCH {
            return false;
        }
        if location.register != source {
            self.output
                .instructions
                .push(Instruction::move_register(location.register, source));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 12,
            member_type: Type::Int,
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_a_compared_member_selected_on_the_false_edge() {
        let value = member();
        let condition = Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(value.clone()),
            right: Box::new(Expression::IntegerLiteral(3)),
        };
        let then_body = [Statement::Assign {
            name: "result".into(),
            value: Expression::IntegerLiteral(1),
        }];
        let else_body = [Statement::Assign {
            name: "result".into(),
            value,
        }];

        assert!(member_else_reuse_plan(&condition, &then_body, &else_body)
            .is_some_and(|plan| plan.result == "result" && plan.base == "object"));
    }

    #[test]
    fn finds_the_immediate_comparison_source() {
        let instructions = [
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 12,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 3 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            },
        ];

        assert_eq!(compared_register_before_branch(&instructions, 2), Some(0));
    }
}
