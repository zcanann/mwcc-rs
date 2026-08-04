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

// Switch-arm exits retain their semantic provenance until the generic
// conditional-goto fold has run. This tagged range cannot collide with a real
// instruction index, and MachineFunction's encoder rejects any tag that leaks
// past the dedicated resolver.
const STRUCTURED_SWITCH_JOIN_PLACEHOLDER: usize = usize::MAX / 4;
// Keep the tag well below the early-return placeholder at `usize::MAX / 2`.
// Instruction scheduling can shift either tagged target by a small amount;
// claiming the entire interval between them would misidentify a shifted
// epilogue branch as a switch join.
const STRUCTURED_SWITCH_JOIN_LIMIT: usize =
    STRUCTURED_SWITCH_JOIN_PLACEHOLDER + usize::MAX / 16;

pub(super) fn lower_structured_switches(function: &Function) -> Option<Function> {
    lower_structured_switches_with_mode(function, false)
}

pub(crate) fn hidden_label_count_with_switches(function: &Function) -> u32 {
    let lowered;
    let statements = if let Some(function) = lower_structured_switches(function) {
        lowered = function;
        &lowered.statements
    } else {
        &function.statements
    };
    super::structured::structured_hidden_label_count(statements)
}

pub(crate) fn nested_retained_switch_hidden_label_count(function: &Function) -> u32 {
    fn statements(function: &Function, body: &[Statement], inside_switch: bool) -> u32 {
        body.iter()
            .map(|statement| match statement {
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    statements(function, then_body, inside_switch)
                        + statements(function, else_body, inside_switch)
                }
                Statement::Loop { body, .. } => {
                    statements(function, body, inside_switch)
                }
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    let retained = u32::from(
                        inside_switch
                            && super::structured_sparse_switch::
                                is_sparse_retained_switch(arms),
                    ) * canonical_switch_hidden_label_count(
                        function,
                        scrutinee,
                        arms,
                        default.as_ref(),
                    );
                    retained
                        + arms
                            .iter()
                            .map(|arm| match &arm.body {
                                ArmBody::Statements(body) => {
                                    statements(function, body, true)
                                }
                                ArmBody::Return(_) => 0,
                            })
                            .sum::<u32>()
                        + default.as_ref().map_or(0, |body| match body {
                            ArmBody::Statements(body) => {
                                statements(function, body, true)
                            }
                            ArmBody::Return(_) => 0,
                        })
                }
                _ => 0,
            })
            .sum()
    }

    statements(function, &function.statements, false)
}

/// Labels by which an outer dense-switch pool walk observes a nested retained
/// sparse dispatch. The nested switch owns one label per explicit case, one
/// for its default edge when present, plus the dispatch and shared join. Its
/// arm-internal optimizer labels remain local to the nested owner and must not
/// advance the outer jump-table ordinal.
pub(crate) fn nested_retained_switch_dispatch_label_count(function: &Function) -> u32 {
    fn statements(body: &[Statement], inside_switch: bool) -> u32 {
        body.iter()
            .map(|statement| match statement {
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    statements(then_body, inside_switch)
                        + statements(else_body, inside_switch)
                }
                Statement::Loop { body, .. } => statements(body, inside_switch),
                Statement::Switch { arms, default, .. } => {
                    let retained = if inside_switch
                        && super::structured_sparse_switch::is_sparse_retained_switch(arms)
                    {
                        arms.len() as u32 + u32::from(default.is_some()) + 2
                    } else {
                        0
                    };
                    retained
                        + arms
                            .iter()
                            .map(|arm| match &arm.body {
                                ArmBody::Statements(body) => statements(body, true),
                                ArmBody::Return(_) => 0,
                            })
                            .sum::<u32>()
                        + default.as_ref().map_or(0, |body| match body {
                            ArmBody::Statements(body) => statements(body, true),
                            ArmBody::Return(_) => 0,
                        })
                }
                _ => 0,
            })
            .sum()
    }

    statements(&function.statements, false)
}

/// Build the structured emitter's control-flow view.
///
/// Analysis still consumes the fully canonicalized if-tree returned by
/// [`lower_structured_switches`].  Dense switches need to retain their source
/// arms for code emission, however: cloning a fallthrough continuation into an
/// if-tree destroys shared case bodies and makes a real jump table impossible.
/// Keeping the two views separate lets liveness remain single-representation
/// while the dispatch owner sees the source topology it must materialize.
pub(super) fn lower_structured_switches_for_emission(
    function: &Function,
) -> Option<Function> {
    lower_structured_switches_with_mode(function, true)
}

fn lower_structured_switches_with_mode(
    function: &Function,
    preserve_dense: bool,
) -> Option<Function> {
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
        preserve_dense,
        control_depth: 0,
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
    let placeholder = STRUCTURED_SWITCH_JOIN_PLACEHOLDER
        .checked_add(join)
        .expect("a structured switch join fits in the placeholder range");
    assert!(
        placeholder < STRUCTURED_SWITCH_JOIN_LIMIT,
        "a structured switch join fits in the reserved tag band"
    );
    placeholder
}

pub(super) fn is_structured_switch_join_placeholder(target: usize) -> bool {
    (STRUCTURED_SWITCH_JOIN_PLACEHOLDER..STRUCTURED_SWITCH_JOIN_LIMIT).contains(&target)
}

pub(super) fn resolve_structured_switch_joins(
    instructions: &mut [mwcc_machine_code::Instruction],
) {
    for instruction in instructions {
        match instruction {
            mwcc_machine_code::Instruction::Branch { target }
            | mwcc_machine_code::Instruction::BranchConditionalForward { target, .. }
                if is_structured_switch_join_placeholder(*target) =>
            {
                *target -= STRUCTURED_SWITCH_JOIN_PLACEHOLDER;
            }
            _ => {}
        }
    }
}

pub(super) fn canonical_switch_hidden_label_count(
    function: &Function,
    scrutinee: &Expression,
    arms: &[mwcc_syntax_trees::SwitchArm],
    default: Option<&ArmBody>,
) -> u32 {
    let mut probe = function.clone();
    probe.statements = vec![Statement::Switch {
        scrutinee: scrutinee.clone(),
        arms: arms.to_vec(),
        default: default.cloned(),
    }];
    lower_structured_switches(&probe)
        .as_ref()
        .map_or(0, |lowered| {
            super::structured::structured_hidden_label_count(&lowered.statements)
        })
}

struct SwitchLowering {
    occupied: HashSet<String>,
    next_switch: usize,
    locals: Vec<LocalDeclaration>,
    changed: bool,
    preserve_dense: bool,
    control_depth: usize,
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
                    then_body: self.lower_nested_statements(then_body),
                    else_body: self.lower_nested_statements(else_body),
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
                    body: self.lower_nested_statements(body),
                }),
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    if self.preserve_dense
                        && (((self.control_depth == 0
                            || is_dense_structured_switch(arms))
                            && (is_dense_structured_switch(arms)
                                || (self.control_depth == 0
                                    && shared_base_comparison_switch(arms).is_some())))
                            || ((self.control_depth == 0
                                || arms.iter().any(|arm| arm.falls_through)
                                || arms.len() >= 4)
                                && super::structured_sparse_switch::is_sparse_retained_switch(arms)))
                    {
                        let switch_has_break = arms.iter().any(|arm| {
                            matches!(&arm.body, ArmBody::Statements(body) if current_switch_has_break(body))
                        }) || matches!(default, Some(ArmBody::Statements(body)) if current_switch_has_break(body));
                        let join_label = switch_has_break.then(|| self.fresh_name());
                        let arms = arms
                            .iter()
                            .map(|arm| mwcc_syntax_trees::SwitchArm {
                                value: arm.value,
                                body: ArmBody::Statements(
                                    self.lower_canonical_arm(
                                        &arm.body,
                                        join_label.as_deref(),
                                    ),
                                ),
                                falls_through: arm.falls_through,
                            })
                            .collect();
                        let default = default
                            .as_ref()
                            .map(|body| {
                                self.lower_canonical_arm(
                                    body,
                                    join_label.as_deref(),
                                )
                            });
                        lowered.push(Statement::Switch {
                            scrutinee: scrutinee.clone(),
                            arms,
                            default: default.map(ArmBody::Statements),
                        });
                        if let Some(join_label) = join_label {
                            lowered.push(Statement::Label(join_label));
                            self.changed = true;
                        }
                        continue;
                    }
                    let mut seen = HashSet::new();
                    if !arms.iter().all(|arm| seen.insert(arm.value)) {
                        lowered.push(statement.clone());
                        continue;
                    }
                    let switch_has_break = arms.iter().any(|arm| {
                        matches!(&arm.body, ArmBody::Statements(body) if current_switch_has_break(body))
                    }) || matches!(default, Some(ArmBody::Statements(body)) if current_switch_has_break(body));
                    let join_label = switch_has_break.then(|| self.fresh_name());
                    let default = default
                        .as_ref()
                        .map_or_else(Vec::new, |body| {
                            self.lower_canonical_arm(body, join_label.as_deref())
                        });
                    // A final fallthrough arm enters the explicit default body.
                    // Earlier fallthrough labels inherit the complete next arm,
                    // which may itself already include that default continuation.
                    let mut continuation = default.clone();
                    let mut cases = Vec::with_capacity(arms.len());
                    for arm in arms.iter().rev() {
                        let mut body = self.lower_canonical_arm(
                            &arm.body,
                            join_label.as_deref(),
                        );
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
                        attribute_alignment: None,
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
                    if let Some(join_label) = join_label {
                        lowered.push(Statement::Label(join_label));
                    }
                    self.changed = true;
                }
                _ => lowered.push(statement.clone()),
            }
        }
        lowered
    }

    fn lower_canonical_arm(
        &mut self,
        body: &ArmBody,
        join_label: Option<&str>,
    ) -> Vec<Statement> {
        match body {
            ArmBody::Statements(statements) => {
                let rewritten = join_label
                    .map(|join_label| rewrite_current_switch_breaks(statements, join_label));
                self.lower_nested_statements(
                    rewritten.as_deref().unwrap_or(statements),
                )
            }
            ArmBody::Return(value) => vec![Statement::Return(Some(value.clone()))],
        }
    }

    fn lower_nested_statements(
        &mut self,
        statements: &[Statement],
    ) -> Vec<Statement> {
        self.control_depth += 1;
        let lowered = self.lower_statements(statements);
        self.control_depth -= 1;
        lowered
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

fn current_switch_has_break(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Break => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => current_switch_has_break(then_body) || current_switch_has_break(else_body),
        // These introduce their own break target. Nested switches are lowered
        // recursively and receive an independent join label.
        Statement::Loop { .. } | Statement::Switch { .. } => false,
        _ => false,
    })
}

fn rewrite_current_switch_breaks(
    statements: &[Statement],
    join_label: &str,
) -> Vec<Statement> {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Break => Statement::Goto(join_label.into()),
            Statement::If {
                condition,
                then_body,
                else_body,
            } => Statement::If {
                condition: condition.clone(),
                then_body: rewrite_current_switch_breaks(then_body, join_label),
                else_body: rewrite_current_switch_breaks(else_body, join_label),
            },
            // Preserve loop breaks for loop lowering and nested switch breaks
            // for the recursive switch invocation.
            Statement::Loop { .. } | Statement::Switch { .. } => statement.clone(),
            _ => statement.clone(),
        })
        .collect()
}

pub(super) fn is_dense_structured_switch(
    arms: &[mwcc_syntax_trees::SwitchArm],
) -> bool {
    if arms.len() < 5 {
        return false;
    }
    let mut values = HashSet::with_capacity(arms.len());
    let Some((minimum, maximum)) = arms.iter().try_fold(
        (i64::MAX, i64::MIN),
        |(minimum, maximum), arm| {
            values
                .insert(arm.value)
                .then_some((minimum.min(arm.value), maximum.max(arm.value)))
        },
    ) else {
        return false;
    };
    let Some(span) = maximum
        .checked_sub(minimum)
        .and_then(|difference| difference.checked_add(1))
    else {
        return false;
    };
    span > 6
        && span <= i64::from(u16::MAX) + 1
        && span <= (arms.len() as i64).saturating_mul(2)
}

/// Return the low-half-zero anchor used by MWCC's comparison tree for a small
/// switch whose 32-bit case values cannot be encoded by `cmpwi`.
///
/// Keeping this policy separate from [`is_dense_structured_switch`] prevents a
/// six-case tree from being mistaken for a jump table merely because its
/// absolute values are large.
pub(super) fn shared_base_comparison_switch(
    arms: &[mwcc_syntax_trees::SwitchArm],
) -> Option<i64> {
    if !(2..=6).contains(&arms.len()) {
        return None;
    }
    let mut values = HashSet::with_capacity(arms.len());
    if !arms.iter().all(|arm| values.insert(arm.value)) {
        return None;
    }
    let minimum = arms.iter().map(|arm| arm.value).min()?;
    if (i16::MIN as i64..i16::MAX as i64).contains(&minimum) {
        return None;
    }
    let base = minimum & !0xffff;
    arms.iter()
        .all(|arm| {
            (i16::MIN as i64..=i16::MAX as i64).contains(&(arm.value - base))
        })
        .then_some(base)
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

    fn arm(value: i64) -> SwitchArm {
        SwitchArm {
            value,
            body: ArmBody::Statements(vec![Statement::Break]),
            falls_through: false,
        }
    }

    #[test]
    fn counts_only_dispatch_labels_for_a_nested_sparse_switch() {
        let nested = Statement::Switch {
            scrutinee: Expression::Variable("command".into()),
            arms: [1, 4, 14, 15]
                .into_iter()
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(vec![Statement::Break]),
                    falls_through: false,
                })
                .collect(),
            default: Some(ArmBody::Statements(vec![Statement::Break])),
        };
        let outer = Statement::Switch {
            scrutinee: Expression::Variable("state".into()),
            arms: vec![SwitchArm {
                value: 0,
                body: ArmBody::Statements(vec![nested]),
                falls_through: false,
            }],
            default: None,
        };

        assert_eq!(
            nested_retained_switch_dispatch_label_count(&function(vec![outer])),
            7
        );
    }

    #[test]
    fn switch_join_tag_does_not_claim_a_shifted_epilogue_placeholder() {
        let shifted_epilogue = usize::MAX / 2 - 4;

        assert!(!is_structured_switch_join_placeholder(shifted_epilogue));
        assert!(is_structured_switch_join_placeholder(
            structured_switch_join_placeholder(12)
        ));
    }

    #[test]
    fn separates_a_small_shared_base_tree_from_dense_jump_tables() {
        let arms = (0..6)
            .map(|offset| arm(0xdcd1_0000 + offset))
            .collect::<Vec<_>>();

        assert_eq!(
            shared_base_comparison_switch(&arms),
            Some(0xdcd1_0000)
        );
        assert!(!is_dense_structured_switch(&arms));
    }

    #[test]
    fn recognizes_a_five_case_seven_entry_jump_table() {
        let arms = [0x700, 0x702, 0x704, 0x705, 0x706]
            .into_iter()
            .map(arm)
            .collect::<Vec<_>>();

        assert!(is_dense_structured_switch(&arms));
    }

    #[test]
    fn rejects_shared_base_offsets_that_do_not_fit_addi() {
        let arms = vec![arm(0xdcd1_0000), arm(0xdcd1_8000)];

        assert_eq!(shared_base_comparison_switch(&arms), None);
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
    fn retains_a_nested_sparse_shared_body_switch_for_emission() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![
                SwitchArm {
                    value: 0,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 16,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "count".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
                SwitchArm {
                    value: 1,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 17,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "range".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
            ],
            default: None,
        };
        let function = function(vec![Statement::If {
            condition: Expression::Variable("ready".into()),
            then_body: vec![switch],
            else_body: Vec::new(),
        }]);

        assert!(lower_structured_switches(&function).is_some());
        assert!(lower_structured_switches_for_emission(&function).is_none());
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

    #[test]
    fn lowers_a_conditional_switch_break_to_its_join() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![SwitchArm {
                value: 1,
                body: ArmBody::Statements(vec![
                    Statement::If {
                        condition: Expression::Variable("done".into()),
                        then_body: vec![Statement::Break],
                        else_body: Vec::new(),
                    },
                    Statement::Store {
                        target: Expression::Variable("result".into()),
                        value: Expression::IntegerLiteral(7),
                    },
                ]),
                falls_through: false,
            }],
            default: None,
        };
        let lowered = lower_structured_switches(&function(vec![switch]))
            .expect("switch break should lower");
        assert!(matches!(
            lowered.statements.as_slice(),
            [
                Statement::Assign { .. },
                Statement::If { then_body, .. },
                Statement::Label(join),
            ] if matches!(then_body.as_slice(), [
                Statement::If { then_body: break_body, .. },
                Statement::Store { .. },
            ] if matches!(break_body.as_slice(), [Statement::Goto(target)] if target == join))
        ));
    }

    #[test]
    fn leaves_a_nested_loop_break_for_loop_lowering() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![SwitchArm {
                value: 1,
                body: ArmBody::Statements(vec![Statement::Loop {
                    kind: mwcc_syntax_trees::LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::IntegerLiteral(1)),
                    step: None,
                    body: vec![Statement::Break],
                }]),
                falls_through: false,
            }],
            default: None,
        };
        let lowered = lower_structured_switches(&function(vec![switch]))
            .expect("switch should lower without claiming the loop break");
        assert!(matches!(
            lowered.statements.as_slice(),
            [Statement::Assign { .. }, Statement::If { then_body, .. }]
                if matches!(then_body.as_slice(), [
                    Statement::Loop { body, .. }
                ] if matches!(body.as_slice(), [Statement::Break]))
        ));
    }

    #[test]
    fn emission_retains_a_dense_eight_case_switch() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: (0..8)
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(vec![Statement::Return(None)]),
                    falls_through: false,
                })
                .collect(),
            default: None,
        };
        let source = function(vec![switch]);
        assert!(
            lower_structured_switches_for_emission(&source).is_none(),
            "a retained source switch needs no rewritten emission function"
        );
        assert!(lower_structured_switches(&source).is_some());
    }

    #[test]
    fn emission_retains_a_dense_seven_case_switch_with_one_hole() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: [0, 1, 2, 3, 4, 5, 7]
                .into_iter()
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(vec![Statement::Return(None)]),
                    falls_through: false,
                })
                .collect(),
            default: None,
        };
        assert!(
            lower_structured_switches_for_emission(&function(vec![switch]))
                .is_none(),
            "a dense source switch retains its one default table entry"
        );
    }

    #[test]
    fn emission_retains_a_dense_switch_nested_in_an_if() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: (0..8)
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(vec![Statement::Return(None)]),
                    falls_through: false,
                })
                .collect(),
            default: None,
        };
        let nested = Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![switch],
            else_body: Vec::new(),
        };
        assert!(
            lower_structured_switches_for_emission(&function(vec![nested]))
                .is_none(),
            "nested dense switches retain the source topology for dispatch"
        );
    }

    #[test]
    fn emission_retains_a_nested_shared_body_switch() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![
                SwitchArm {
                    value: 4,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 5,
                    body: ArmBody::Statements(vec![Statement::Return(None)]),
                    falls_through: false,
                },
            ],
            default: None,
        };
        let nested = Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![switch],
            else_body: Vec::new(),
        };

        assert!(
            lower_structured_switches_for_emission(&function(vec![nested])).is_none(),
            "the emission view must not clone a nested shared body"
        );
    }

    #[test]
    fn emission_rewrites_breaks_in_a_retained_shared_body_switch() {
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("kind".into()),
            arms: vec![
                SwitchArm {
                    value: 4,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 5,
                    body: ArmBody::Statements(vec![Statement::If {
                        condition: Expression::Variable("done".into()),
                        then_body: vec![Statement::Break],
                        else_body: Vec::new(),
                    }]),
                    falls_through: false,
                },
            ],
            default: None,
        };

        let lowered = lower_structured_switches_for_emission(&function(vec![switch]))
            .expect("a retained switch break needs an explicit join");
        assert!(matches!(
            lowered.statements.as_slice(),
            [Statement::Switch { arms, .. }, Statement::Label(join)]
                if matches!(
                    &arms[1].body,
                    ArmBody::Statements(body)
                        if matches!(
                            body.as_slice(),
                            [Statement::If { then_body, .. }]
                                if matches!(
                                    then_body.as_slice(),
                                    [Statement::Goto(target)] if target == join
                                )
                        )
                )
        ));
    }
}
