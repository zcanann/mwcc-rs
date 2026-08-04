//! Saved-GPR windows for high-pressure structured loops.
//!
//! MWCC gives each scalar role used by one lexical loop a loop-wide allocation
//! interval.  Retained call homes occupy the same register bank, and only the
//! ten volatile GPRs (`r3..r12`) absorb that combined pressure for free.  The
//! remaining roles form a descending saved-GPR suffix.  Frame planning needs
//! that suffix width before virtual-register allocation: discovering pressure
//! later can grow a dense range, but cannot change an individually saved frame
//! into the helper-based contiguous form.

#[allow(unused_imports)]
use super::*;

use super::structured_locals::body_uses_local;
use mwcc_syntax_trees::Parameter;
use std::collections::HashSet;

pub(super) const DENSE_SAVED_GPR_COUNT: usize = 18;
const VOLATILE_GPR_COUNT: usize = 10;

const DENSE_LOOP_CARRIED_REGISTERS: [u8; 4] = [30, 29, 28, 27];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DenseLoopCarriedPlan<'a> {
    locals: [Option<&'a str>; DENSE_LOOP_CARRIED_REGISTERS.len()],
}

impl DenseLoopCarriedPlan<'_> {
    pub(super) fn preference_for(&self, local: &str) -> Option<u8> {
        self.locals
            .iter()
            .position(|candidate| *candidate == Some(local))
            .map(|rank| DENSE_LOOP_CARRIED_REGISTERS[rank])
    }
}

/// Plan the measured descending homes for values carried around one saturated
/// loop. Packet scheduling is coupled to these lanes, so this bounded plan only
/// exposes roles whose interaction has been verified.
pub(super) fn plan_dense_loop_carried_locals<'a>(
    statements: &[Statement],
    ephemeral_locals: &[&'a LocalDeclaration],
    saved_window: Option<usize>,
) -> DenseLoopCarriedPlan<'a> {
    let mut plan = DenseLoopCarriedPlan {
        locals: [None; DENSE_LOOP_CARRIED_REGISTERS.len()],
    };
    if saved_window != Some(DENSE_SAVED_GPR_COUNT) {
        return plan;
    }
    let Some(loop_statement) = dense_loop_statement(statements, ephemeral_locals) else {
        return plan;
    };
    for (slot, local) in plan.locals.iter_mut().zip(
        ephemeral_locals
            .iter()
            .filter(|local| class_of(local.declared_type).ok() == Some(ValueClass::General))
            .filter(|local| loop_carries_name(loop_statement, &local.name)),
    ) {
        *slot = Some(local.name.as_str());
    }
    plan
}

/// Return the saved-home count for a source loop that saturates MWCC's saved
/// GPR window. Locals are counted per lexical loop, so unrelated temporaries in
/// separate loops cannot accidentally combine into a dense-frame signal.
pub(super) fn plan_dense_loop_register_window(
    statements: &[Statement],
    ephemeral_locals: &[&LocalDeclaration],
) -> Option<usize> {
    statements.iter().find_map(|statement| match statement {
        Statement::Loop { body, .. } => {
            let general_locals = ephemeral_locals
                .iter()
                .filter(|local| {
                    class_of(local.declared_type).ok() == Some(ValueClass::General)
                        && body_uses_local(std::slice::from_ref(statement), &local.name)
                })
                .count();
            (general_locals >= DENSE_SAVED_GPR_COUNT)
                .then_some(DENSE_SAVED_GPR_COUNT)
                .or_else(|| plan_dense_loop_register_window(body, ephemeral_locals))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => plan_dense_loop_register_window(then_body, ephemeral_locals)
            .or_else(|| plan_dense_loop_register_window(else_body, ephemeral_locals)),
        _ => None,
    })
}

/// Return the complete saved suffix required by the highest-pressure lexical
/// loop after accounting for call-retained homes already present in the frame.
///
/// The older complete-window signal above remains intentionally separate: it
/// gates a narrow frame-publication shape before retained homes have been
/// planned.  This calculation owns the ordinary frame width once that retained
/// count is known.
pub(super) fn plan_dense_loop_saved_register_window(
    function: &Function,
    ephemeral_locals: &[&LocalDeclaration],
    retained_home_count: usize,
    retained_parameters: &[&Parameter],
) -> Option<usize> {
    let loop_role_count = maximum_loop_register_role_count(
        &function.statements,
        ephemeral_locals,
        &function.locals,
    )?;
    let forwarded_parameter_homes = preloop_forwarded_parameter_home_count(
        function,
        ephemeral_locals,
        retained_parameters,
    );
    saved_register_window(
        loop_role_count,
        retained_home_count,
        forwarded_parameter_homes,
    )
}

fn saved_register_window(
    loop_role_count: usize,
    retained_home_count: usize,
    forwarded_parameter_home_count: usize,
) -> Option<usize> {
    let retained_pressure = retained_home_count
        .saturating_sub(forwarded_parameter_home_count.min(retained_home_count));
    let total_pressure = loop_role_count.checked_add(retained_pressure)?;
    let saved_count = total_pressure
        .saturating_sub(VOLATILE_GPR_COUNT)
        .min(DENSE_SAVED_GPR_COUNT);
    (saved_count > retained_home_count).then_some(saved_count)
}

fn maximum_loop_register_role_count(
    statements: &[Statement],
    ephemeral_locals: &[&LocalDeclaration],
    function_locals: &[LocalDeclaration],
) -> Option<usize> {
    statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Loop { body, .. } => {
                let general_locals = ephemeral_locals
                    .iter()
                    .filter(|local| {
                        class_of(local.declared_type).ok() == Some(ValueClass::General)
                            && body_uses_local(std::slice::from_ref(statement), &local.name)
                    })
                    .count();
                let automatic_array_addresses = function_locals
                    .iter()
                    .filter(|local| {
                        !local.is_static
                            && local.array_length.is_some()
                            && body_uses_local(std::slice::from_ref(statement), &local.name)
                    })
                    .count();
                let loop_roles = general_locals.checked_add(automatic_array_addresses)?;
                Some(
                    maximum_loop_register_role_count(body, ephemeral_locals, function_locals)
                        .map_or(loop_roles, |nested| loop_roles.max(nested)),
                )
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => maximum_loop_register_role_count(
                then_body,
                ephemeral_locals,
                function_locals,
            )
                .into_iter()
                .chain(maximum_loop_register_role_count(
                    else_body,
                    ephemeral_locals,
                    function_locals,
                ))
                .max(),
            _ => None,
        })
        .max()
}

/// Count retained parameter homes whose incoming value is renamed into an
/// ephemeral loop role before the first top-level loop. The renamed role is
/// already present in `loop_role_count`, so retaining both would count one
/// physical interval twice (`pbyPcmData -> pSrc` and `pbyAdpcmData -> pDst` in
/// WENC). Nested-loop prefixes remain conservative until their CFG path is
/// available here.
fn preloop_forwarded_parameter_home_count(
    function: &Function,
    ephemeral_locals: &[&LocalDeclaration],
    retained_parameters: &[&Parameter],
) -> usize {
    let prefix = function
        .statements
        .iter()
        .take_while(|statement| !matches!(statement, Statement::Loop { .. }));
    let mut forwarded = HashSet::new();
    for statement in prefix {
        let Statement::Assign {
            name,
            value: Expression::Variable(source),
        } = statement
        else {
            continue;
        };
        if ephemeral_locals.iter().any(|local| {
            local.name == *name
                && class_of(local.declared_type).ok() == Some(ValueClass::General)
        }) && retained_parameters
            .iter()
            .any(|parameter| parameter.name == *source)
        {
            forwarded.insert(source.as_str());
        }
    }
    forwarded.len()
}

fn dense_loop_statement<'a>(
    statements: &'a [Statement],
    ephemeral_locals: &[&LocalDeclaration],
) -> Option<&'a Statement> {
    statements.iter().find_map(|statement| match statement {
        Statement::Loop { body, .. } => {
            let general_locals = ephemeral_locals
                .iter()
                .filter(|local| {
                    class_of(local.declared_type).ok() == Some(ValueClass::General)
                        && body_uses_local(std::slice::from_ref(statement), &local.name)
                })
                .count();
            (general_locals >= DENSE_SAVED_GPR_COUNT)
                .then_some(statement)
                .or_else(|| dense_loop_statement(body, ephemeral_locals))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => dense_loop_statement(then_body, ephemeral_locals)
            .or_else(|| dense_loop_statement(else_body, ephemeral_locals)),
        _ => None,
    })
}

fn loop_carries_name(statement: &Statement, name: &str) -> bool {
    let Statement::Loop {
        condition,
        step,
        body,
        ..
    } = statement
    else {
        return false;
    };
    sequence_assigns_name(body, name)
        && (condition
            .as_ref()
            .is_some_and(|condition| expression_reads_name(condition, name))
            || reads_before_assignment(body, name)
            || step
                .as_ref()
                .is_some_and(|step| expression_reads_name(step, name)))
}

fn sequence_assigns_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => sequence_assigns_name(then_body, name) || sequence_assigns_name(else_body, name),
        Statement::Loop { body, .. } => sequence_assigns_name(body, name),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| arm_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_assigns_name(body, name))
        }
        _ => false,
    })
}

fn arm_assigns_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(_) => false,
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            sequence_assigns_name(statements, name)
        }
    }
}

fn reads_before_assignment(statements: &[Statement], name: &str) -> bool {
    for statement in statements {
        match statement {
            Statement::Assign {
                name: assigned,
                value,
            } if assigned == name => return expression_reads_name(value, name),
            Statement::Assign { value, .. }
                if scalar_assignment_chain_defines_without_read(value, name) =>
            {
                return false;
            }
            _ if statement_references_name(statement, name) => return true,
            _ => {}
        }
    }
    false
}

/// Whether a right-nested scalar assignment chain gives `name` a fresh value
/// before any read of its incoming value.
///
/// The parser represents `a = b = c = 0` as one outer statement whose value
/// contains the `b` and `c` definitions.  Treating those target occurrences as
/// reads makes iteration-local flags look loop-carried and gives them saved-GPR
/// preferences.  Restrict this proof to the pure scalar chain; other expression
/// shapes retain the conservative reference scan above.
fn scalar_assignment_chain_defines_without_read(expression: &Expression, name: &str) -> bool {
    let Expression::Assign { target, value } = expression else {
        return false;
    };
    let Expression::Variable(assigned) = target.as_ref() else {
        return false;
    };
    if assigned == name {
        return !expression_reads_name(value, name);
    }
    scalar_assignment_chain_defines_without_read(value, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::UnsignedInt,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn read(name: &str) -> Statement {
        Statement::Expression(Expression::Variable(name.into()))
    }

    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.into(),
            value,
        }
    }

    #[test]
    fn reserves_the_complete_window_for_one_saturated_loop() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: locals.iter().map(|local| read(&local.name)).collect(),
        }];

        assert_eq!(
            plan_dense_loop_register_window(&statements, &references),
            Some(DENSE_SAVED_GPR_COUNT)
        );
    }

    #[test]
    fn converts_loop_roles_and_retained_homes_into_one_saved_suffix() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: locals.iter().map(|local| read(&local.name)).collect(),
        }];

        assert_eq!(
            saved_register_window(
                maximum_loop_register_role_count(&statements, &references, &[]).unwrap(),
                5,
                0,
            ),
            Some(13)
        );
    }

    #[test]
    fn coalesces_forwarded_parameter_homes_and_counts_an_array_address_role() {
        assert_eq!(saved_register_window(19, 5, 2), Some(12));
        assert_eq!(saved_register_window(20, 5, 2), Some(13));
    }

    #[test]
    fn leaves_a_retained_home_plan_alone_below_volatile_capacity() {
        let locals: Vec<_> = (0..6)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: locals.iter().map(|local| read(&local.name)).collect(),
        }];

        assert_eq!(
            saved_register_window(
                maximum_loop_register_role_count(&statements, &references, &[]).unwrap(),
                4,
                0,
            ),
            None
        );
    }

    #[test]
    fn caps_loop_pressure_at_the_architectural_saved_bank() {
        let locals: Vec<_> = (0..30)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: locals.iter().map(|local| read(&local.name)).collect(),
        }];

        assert_eq!(
            saved_register_window(
                maximum_loop_register_role_count(&statements, &references, &[]).unwrap(),
                3,
                0,
            ),
            Some(DENSE_SAVED_GPR_COUNT)
        );
    }

    #[test]
    fn does_not_combine_pressure_from_separate_loops() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let statements = vec![
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: locals[..9].iter().map(|local| read(&local.name)).collect(),
            },
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: locals[9..].iter().map(|local| read(&local.name)).collect(),
            },
        ];

        assert_eq!(
            plan_dense_loop_register_window(&statements, &references),
            None
        );
        assert_eq!(
            saved_register_window(
                maximum_loop_register_role_count(&statements, &references, &[]).unwrap(),
                0,
                0,
            ),
            None
        );
    }

    #[test]
    fn selects_the_first_carried_value_in_a_saturated_loop() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let mut body = vec![
            read("v0"),
            assign(
                "v0",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v0".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
            assign("v1", Expression::IntegerLiteral(7)),
            read("v1"),
            read("v2"),
            assign(
                "v2",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v2".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
            read("v3"),
            assign(
                "v3",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v3".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
            read("v4"),
            assign(
                "v4",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v4".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
        ];
        body.extend(locals[5..].iter().map(|local| read(&local.name)));
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body,
        }];

        let plan = plan_dense_loop_carried_locals(
            &statements,
            &references,
            Some(DENSE_SAVED_GPR_COUNT),
        );
        assert_eq!(
            plan.locals,
            [Some("v0"), Some("v2"), Some("v3"), Some("v4")]
        );
        assert_eq!(plan.preference_for("v0"), Some(30));
        assert_eq!(plan.preference_for("v4"), Some(27));
        assert_eq!(plan.preference_for("v5"), None);
    }

    #[test]
    fn chained_iteration_resets_are_not_loop_carried() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let reset = Statement::Assign {
            name: "v17".into(),
            value: Expression::Assign {
                target: Box::new(Expression::Variable("v0".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            },
        };
        let mut body = vec![reset, read("v0")];
        body.extend(locals[1..].iter().map(|local| read(&local.name)));
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body,
        }];

        let plan = plan_dense_loop_carried_locals(
            &statements,
            &references,
            Some(DENSE_SAVED_GPR_COUNT),
        );
        assert_eq!(plan.preference_for("v0"), None);
    }

    #[test]
    fn does_not_prefer_carried_roles_in_a_partial_saved_window() {
        let locals: Vec<_> = (0..DENSE_SAVED_GPR_COUNT)
            .map(|index| local(&format!("v{index}")))
            .collect();
        let references: Vec<_> = locals.iter().collect();
        let mut body = vec![
            read("v0"),
            assign(
                "v0",
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("v0".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            ),
        ];
        body.extend(locals[1..].iter().map(|local| read(&local.name)));
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body,
        }];

        let plan = plan_dense_loop_carried_locals(&statements, &references, Some(13));
        assert_eq!(plan.preference_for("v0"), None);
    }
}
