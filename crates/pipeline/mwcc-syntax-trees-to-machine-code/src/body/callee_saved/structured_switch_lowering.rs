//! Canonical CFG lowering for switches owned by the structured body emitter.
//!
//! Structured liveness and definite-assignment planning already understand
//! nested `if` trees. A non-fallthrough switch is the same control-flow shape
//! after evaluating its scrutinee once, so normalize it before those plans run
//! instead of teaching every plan a second branch representation.

use mwcc_syntax_trees::{
    ArmBody, BinaryOperator, Expression, Function, LocalDeclaration, Statement, Type,
};
use std::collections::HashSet;

const STRUCTURED_SWITCH_JOIN_PLACEHOLDER: usize = usize::MAX / 4;
const STRUCTURED_SWITCH_JOIN_LIMIT: usize = usize::MAX / 2;

pub(super) fn lower_structured_switches(function: &Function) -> Option<Function> {
    let occupied = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut lowering = SwitchLowering {
        occupied,
        next_switch: 0,
        locals: function.locals.clone(),
        changed: false,
    };
    let statements = lowering.lower_statements(&function.statements);
    lowering.changed.then(|| {
        let mut lowered = function.clone();
        lowered.locals = lowering.locals;
        lowered.statements = statements;
        lowered
    })
}

pub(super) fn is_lowered_switch_guard(condition: &Expression) -> bool {
    matches!(
        condition,
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name)
            if name.starts_with("__mwcc_structured_switch_"))
            && matches!(right.as_ref(), Expression::IntegerLiteral(_))
    )
}

pub(super) fn structured_switch_join_placeholder(join: usize) -> usize {
    STRUCTURED_SWITCH_JOIN_PLACEHOLDER
        .checked_add(join)
        .expect("a structured switch join fits in the placeholder range")
}

pub(super) fn is_structured_switch_join_placeholder(target: usize) -> bool {
    (STRUCTURED_SWITCH_JOIN_PLACEHOLDER
        ..STRUCTURED_SWITCH_JOIN_LIMIT)
        .contains(&target)
}

pub(super) fn resolve_structured_switch_joins(
    instructions: &mut [mwcc_machine_code::Instruction],
) {
    for instruction in instructions {
        match instruction {
            mwcc_machine_code::Instruction::Branch { target }
            | mwcc_machine_code::Instruction::BranchConditionalForward {
                target,
                ..
            } if is_structured_switch_join_placeholder(*target) => {
                *target -= STRUCTURED_SWITCH_JOIN_PLACEHOLDER;
            }
            _ => {}
        }
    }
}

struct SwitchLowering {
    occupied: HashSet<String>,
    next_switch: usize,
    locals: Vec<LocalDeclaration>,
    changed: bool,
}

impl SwitchLowering {
    fn lower_statements(&mut self, statements: &[Statement]) -> Vec<Statement> {
        let mut lowered = Vec::with_capacity(statements.len());
        for statement in statements {
            match statement {
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => lowered.push(Statement::If {
                    condition: condition.clone(),
                    then_body: self.lower_statements(then_body),
                    else_body: self.lower_statements(else_body),
                }),
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => lowered.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body: self.lower_statements(body),
                }),
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    let mut seen = HashSet::new();
                    if !arms.iter().all(|arm| seen.insert(arm.value)) {
                        lowered.push(statement.clone());
                        continue;
                    }
                    let default = default
                        .as_ref()
                        .map_or_else(Vec::new, |body| self.lower_arm(body));
                    // A final fallthrough arm enters the explicit default body.
                    // Earlier fallthrough labels inherit the complete next arm,
                    // which may itself already include that default continuation.
                    let mut continuation = default.clone();
                    let mut cases = Vec::with_capacity(arms.len());
                    for arm in arms.iter().rev() {
                        let mut body = self.lower_arm(&arm.body);
                        if arm.falls_through {
                            body.extend(continuation.clone());
                        }
                        continuation = body.clone();
                        cases.push((arm.value, body));
                    }
                    cases.sort_by_key(|(value, _)| *value);
                    let name = self.fresh_name();
                    self.locals.push(LocalDeclaration {
                        declared_type: Type::Int,
                        name: name.clone(),
                        initializer: None,
                        is_volatile: false,
                        array_length: None,
                        is_static: false,
                        data_bytes: None,
                        data_relocations: Vec::new(),
                        is_const: false,
                        row_bytes: None,
                    });
                    lowered.push(Statement::Assign {
                        name: name.clone(),
                        value: scrutinee.clone(),
                    });
                    let mut decision = default;
                    for (value, body) in cases.into_iter().rev() {
                        decision = vec![Statement::If {
                            condition: Expression::Binary {
                                operator: BinaryOperator::Equal,
                                left: Box::new(Expression::Variable(name.clone())),
                                right: Box::new(Expression::IntegerLiteral(value)),
                            },
                            then_body: body,
                            else_body: decision,
                        }];
                    }
                    lowered.extend(decision);
                    self.changed = true;
                }
                _ => lowered.push(statement.clone()),
            }
        }
        lowered
    }

    fn lower_arm(&mut self, body: &ArmBody) -> Vec<Statement> {
        match body {
            ArmBody::Statements(statements) => self.lower_statements(statements),
            ArmBody::Return(value) => vec![Statement::Return(Some(value.clone()))],
        }
    }

    fn fresh_name(&mut self) -> String {
        loop {
            let name = format!("__mwcc_structured_switch_{}", self.next_switch);
            self.next_switch += 1;
            if self.occupied.insert(name.clone()) {
                return name;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::SwitchArm;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "dispatch".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
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
    fn lowers_a_non_fallthrough_switch_to_one_evaluated_scrutinee() {
        let switch = Statement::Switch {
            scrutinee: Expression::Call {
                name: "kind".into(),
                arguments: Vec::new(),
            },
            arms: vec![
                SwitchArm {
                    value: 25,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "second".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
                SwitchArm {
                    value: 2,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "first".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
            ],
            default: None,
        };

        let lowered = lower_structured_switches(&function(vec![switch])).expect("lowered switch");
        assert_eq!(lowered.locals.len(), 1);
        assert!(matches!(
            lowered.statements.as_slice(),
            [
                Statement::Assign {
                    value: Expression::Call { name, .. },
                    ..
                },
                Statement::If {
                    condition: Expression::Binary {
                        right,
                        ..
                    },
                    else_body,
                    ..
                },
            ] if name == "kind"
                && matches!(right.as_ref(), Expression::IntegerLiteral(2))
                && matches!(else_body.as_slice(), [
                    Statement::If {
                        condition: Expression::Binary { right, .. },
                        ..
                    }
                ] if matches!(right.as_ref(), Expression::IntegerLiteral(25)))
        ));
    }

    #[test]
    fn leaves_fallthrough_switches_for_a_dedicated_owner() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![SwitchArm {
                value: 1,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            }],
            default: None,
        };
        let lowered =
            lower_structured_switches(&function(vec![switch])).expect("lowered fallthrough");
        assert!(matches!(
            lowered.statements.as_slice(),
            [Statement::Assign { .. }, Statement::If { then_body, .. }]
                if then_body.is_empty()
        ));
    }

    #[test]
    fn carries_a_fallthrough_case_into_the_next_case_body() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![
                SwitchArm {
                    value: 0,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 2,
                    body: ArmBody::Return(Expression::IntegerLiteral(6)),
                    falls_through: false,
                },
            ],
            default: None,
        };
        let lowered =
            lower_structured_switches(&function(vec![switch])).expect("lowered fallthrough");
        assert!(matches!(
            lowered.statements.as_slice(),
            [
                Statement::Assign { .. },
                Statement::If {
                    then_body,
                    else_body,
                    ..
                },
            ] if matches!(then_body.as_slice(), [Statement::Return(Some(Expression::IntegerLiteral(6)))])
                && matches!(
                    else_body.as_slice(),
                    [Statement::If { then_body, .. }]
                        if matches!(
                            then_body.as_slice(),
                            [Statement::Return(Some(Expression::IntegerLiteral(6)))]
                        )
                )
        ));
    }

    #[test]
    fn carries_a_final_fallthrough_arm_into_the_default_body() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![SwitchArm {
                value: 1,
                body: ArmBody::Statements(Vec::new()),
                falls_through: true,
            }],
            default: Some(ArmBody::Return(Expression::IntegerLiteral(2))),
        };
        let lowered =
            lower_structured_switches(&function(vec![switch])).expect("lowered fallthrough");
        assert!(matches!(
            lowered.statements.as_slice(),
            [
                Statement::Assign { .. },
                Statement::If {
                    then_body,
                    else_body,
                    ..
                }
            ] if matches!(
                then_body.as_slice(),
                [Statement::Return(Some(Expression::IntegerLiteral(2)))]
            ) && matches!(
                else_body.as_slice(),
                [Statement::Return(Some(Expression::IntegerLiteral(2)))]
            )
        ));
    }
}
