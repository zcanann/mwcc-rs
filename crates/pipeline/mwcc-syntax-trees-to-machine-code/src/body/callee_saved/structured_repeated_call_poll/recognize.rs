use std::collections::HashSet;

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement};

use super::super::structured_expression_visit::{visit_expression, visit_statement};

pub(super) const MINIMUM_POLL_PAIRS: usize = 6;

pub(in super::super) fn is_repeated_call_poll_transaction(function: &Function) -> bool {
    let mut plan = PollTransaction::default();
    if !plan.visit_block(&function.statements) || plan.pair_count < MINIMUM_POLL_PAIRS {
        return false;
    }
    plan.sender.is_some() && plan.poller.is_some()
}

pub(in super::super) fn owns_long_string_data_anchor(function: &Function) -> bool {
    let mut strings = HashSet::new();
    let mut collect = |expression: &Expression| {
        if let Expression::StringLiteral(bytes) = expression {
            if bytes.len() + 1 > 8 {
                strings.insert(bytes.clone());
            }
        }
    };
    for statement in &function.statements {
        visit_statement(statement, &mut collect);
    }
    if let Some(expression) = &function.return_expression {
        visit_expression(expression, &mut collect);
    }
    strings.len() >= 3
}

#[derive(Default)]
struct PollTransaction<'a> {
    sender: Option<&'a str>,
    poller: Option<&'a str>,
    pair_count: usize,
    leading_poll_count: usize,
}

impl<'a> PollTransaction<'a> {
    fn visit_block(&mut self, statements: &'a [Statement]) -> bool {
        for (index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::Loop { .. } => {
                    let sender = index
                        .checked_sub(1)
                        .and_then(|previous| direct_call(&statements[previous]));
                    let Some((poller, operator)) = empty_call_poll(statement) else {
                        return false;
                    };
                    if let Some(sender) = sender {
                        if operator != BinaryOperator::NotEqual
                            || !same_or_record(&mut self.sender, sender)
                            || !same_or_record(&mut self.poller, poller)
                        {
                            return false;
                        }
                        self.pair_count += 1;
                    } else if operator == BinaryOperator::Equal
                        && self.pair_count == 0
                        && self.leading_poll_count == 0
                    {
                        self.leading_poll_count = 1;
                    } else {
                        return false;
                    }
                }
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    if !self.visit_block(then_body) || !self.visit_block(else_body) {
                        return false;
                    }
                }
                Statement::Switch { arms, default, .. } => {
                    for arm in arms {
                        if let mwcc_syntax_trees::ArmBody::Statements(body) = &arm.body {
                            if !self.visit_block(body) {
                                return false;
                            }
                        }
                    }
                    if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                        if !self.visit_block(body) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }
}

fn same_or_record<'a>(slot: &mut Option<&'a str>, candidate: &'a str) -> bool {
    match slot {
        Some(expected) => *expected == candidate,
        None => {
            *slot = Some(candidate);
            true
        }
    }
}

fn direct_call(statement: &Statement) -> Option<&str> {
    let Statement::Expression(Expression::Call { name, .. }) = statement else {
        return None;
    };
    Some(name)
}

fn empty_call_poll(statement: &Statement) -> Option<(&str, BinaryOperator)> {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition:
            Some(Expression::Binary {
                operator,
                left,
                right,
            }),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    if !matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual)
        || !body.is_empty()
        || !matches!(right.as_ref(), Expression::IntegerLiteral(0))
    {
        return None;
    }
    let Expression::Call { name, arguments } = left.as_ref() else {
        return None;
    };
    arguments.is_empty().then_some((name, *operator))
}
