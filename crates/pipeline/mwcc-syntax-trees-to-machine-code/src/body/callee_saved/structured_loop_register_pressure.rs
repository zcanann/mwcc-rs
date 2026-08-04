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
use std::collections::{HashMap, HashSet};

pub(super) const DENSE_SAVED_GPR_COUNT: usize = 18;
const VOLATILE_GPR_COUNT: usize = 10;

const DENSE_LOOP_CARRIED_REGISTERS: [u8; 4] = [30, 29, 28, 27];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct DenseLoopCarriedPlan<'a> {
    locals: [Option<&'a str>; DENSE_LOOP_CARRIED_REGISTERS.len()],
}

#[derive(Debug, Default)]
pub(super) struct DenseLoopSavedHomePreferences {
    homes: HashMap<usize, u8>,
    forwarded_parameters: HashMap<String, String>,
}

impl DenseLoopSavedHomePreferences {
    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.homes.get(&home_index).copied()
    }

    pub(super) fn forwarded_parameter(&self, local: &str) -> Option<&str> {
        self.forwarded_parameters.get(local).map(String::as_str)
    }
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
    let forwarded_parameter_homes = preloop_forwarded_parameter_homes(
        function,
        ephemeral_locals,
        retained_parameters,
    );
    saved_register_window(
        loop_role_count,
        retained_home_count,
        forwarded_parameter_homes.len(),
    )
}

/// Place loop-long entry values at the high end of a measured saved suffix.
///
/// MWCC orders those values by descending source-parameter position, then puts
/// retained locals below them. Entry-only parameters occupy the low end and
/// can be reused by short-lived loop temporaries. This is the allocation shape
/// behind both the frame-array WENC loop and its table-free reduction; the
/// preference remains non-binding when interference requires another lane.
pub(super) fn plan_dense_loop_saved_home_preferences(
    function: &Function,
    ephemeral_locals: &[&LocalDeclaration],
    saved_window: Option<usize>,
    retained_parameters: &[&Parameter],
    eager_home_count: usize,
    fresh_home_count: usize,
) -> DenseLoopSavedHomePreferences {
    let Some(saved_count) = saved_window else {
        return DenseLoopSavedHomePreferences::default();
    };
    let retained_home_count = eager_home_count
        .checked_add(retained_parameters.len())
        .and_then(|count| count.checked_add(fresh_home_count));
    let Some(retained_home_count) = retained_home_count.filter(|count| *count <= saved_count)
    else {
        return DenseLoopSavedHomePreferences::default();
    };
    let Some(loop_index) = function
        .statements
        .iter()
        .position(|statement| matches!(statement, Statement::Loop { .. }))
    else {
        return DenseLoopSavedHomePreferences::default();
    };
    let loop_and_suffix = &function.statements[loop_index..];
    let forwarded = preloop_forwarded_parameter_homes(
        function,
        ephemeral_locals,
        retained_parameters,
    );
    let mut long_parameters: Vec<_> = retained_parameters
        .iter()
        .enumerate()
        .filter_map(|(retained_index, parameter)| {
            let forwarded_target = forwarded
                .iter()
                .find_map(|(source, target)| (source == &parameter.name).then_some(target));
            let is_loop_long = body_uses_local(loop_and_suffix, &parameter.name)
                || function
                    .return_expression
                    .as_ref()
                    .is_some_and(|value| expression_reads_name(value, &parameter.name))
                || forwarded_target
                    .is_some_and(|target| body_uses_local(loop_and_suffix, target));
            is_loop_long.then(|| {
                let source_index = function
                    .parameters
                    .iter()
                    .position(|candidate| candidate.name == parameter.name)
                    .unwrap_or(0);
                (source_index, eager_home_count + retained_index)
            })
        })
        .collect();
    long_parameters.sort_unstable_by(|left, right| right.0.cmp(&left.0));

    let mut homes = HashMap::new();
    let mut high = 31u8;
    for (_, home_index) in long_parameters {
        homes.insert(home_index, high);
        high = high.saturating_sub(1);
    }
    for home_index in (0..eager_home_count).chain(
        eager_home_count + retained_parameters.len()..retained_home_count,
    ) {
        homes.insert(home_index, high);
        high = high.saturating_sub(1);
    }

    let mut low = u8::try_from(32usize.saturating_sub(saved_count)).unwrap_or(14);
    for retained_index in 0..retained_parameters.len() {
        let home_index = eager_home_count + retained_index;
        if homes.contains_key(&home_index) {
            continue;
        }
        if low > high {
            return DenseLoopSavedHomePreferences::default();
        }
        homes.insert(home_index, low);
        low = low.saturating_add(1);
    }
    let forwarded_parameters = forwarded
        .into_iter()
        .map(|(source, target)| (target, source))
        .collect();
    DenseLoopSavedHomePreferences {
        homes,
        forwarded_parameters,
    }
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

/// Identify retained parameter homes whose incoming value is renamed into an
/// ephemeral loop role before the first top-level loop. The renamed role is
/// already present in `loop_role_count`, so retaining both would count one
/// physical interval twice (`pbyPcmData -> pSrc` and `pbyAdpcmData -> pDst` in
/// WENC). Nested-loop prefixes remain conservative until their CFG path is
/// available here.
fn preloop_forwarded_parameter_homes(
    function: &Function,
    ephemeral_locals: &[&LocalDeclaration],
    retained_parameters: &[&Parameter],
) -> Vec<(String, String)> {
    let loop_index = function
        .statements
        .iter()
        .position(|statement| matches!(statement, Statement::Loop { .. }))
        .unwrap_or(function.statements.len());
    let mut seen = HashSet::new();
    let mut forwarded = Vec::new();
    for (index, statement) in function.statements[..loop_index].iter().enumerate() {
        let Statement::Assign {
            name,
            value: Expression::Variable(source),
        } = statement
        else {
            continue;
        };
        let eligible_target = ephemeral_locals.iter().any(|local| {
            local.name == *name
                && local.initializer.is_none()
                && class_of(local.declared_type).ok() == Some(ValueClass::General)
        });
        let eligible_source = retained_parameters
            .iter()
            .any(|parameter| {
                parameter.name == *source
                    && class_of(parameter.parameter_type).ok() == Some(ValueClass::General)
            });
        let target_unused_before_forward = function.statements[..index]
            .iter()
            .all(|statement| !statement_observes_or_assigns_name(statement, name));
        let source_dead_after_forward = function.statements[index + 1..]
            .iter()
            .all(|statement| !statement_observes_or_assigns_name(statement, source))
            && function.guards.iter().all(|guard| {
                !expression_reads_name(&guard.condition, source)
                    && !expression_assigns_name(&guard.condition, source)
                    && !expression_reads_name(&guard.value, source)
                    && !expression_assigns_name(&guard.value, source)
            })
            && function.return_expression.as_ref().is_none_or(|value| {
                !expression_reads_name(value, source) && !expression_assigns_name(value, source)
            });
        if eligible_target
            && eligible_source
            && target_unused_before_forward
            && source_dead_after_forward
        {
            if seen.insert(source.as_str()) {
                forwarded.push((source.clone(), name.clone()));
            }
        }
    }
    forwarded
}

fn statement_observes_or_assigns_name(statement: &Statement, name: &str) -> bool {
    if super::structured_liveness::statement_reads_name(statement, name) {
        return true;
    }
    match statement {
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || expression_assigns_name(value, name),
        Statement::Store { target, value } => {
            expression_assigns_name(target, name) || expression_assigns_name(value, name)
        }
        Statement::Expression(value) | Statement::Return(Some(value)) => {
            expression_assigns_name(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_assigns_name(condition, name)
                || then_body
                    .iter()
                    .chain(else_body)
                    .any(|statement| statement_observes_or_assigns_name(statement, name))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_assigns_name(scrutinee, name)
                || arms.iter().any(|arm| arm_observes_or_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_observes_or_assigns_name(body, name))
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
                .any(|value| expression_assigns_name(value, name))
                || body
                    .iter()
                    .any(|statement| statement_observes_or_assigns_name(statement, name))
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Return(None) => false,
    }
}

fn arm_observes_or_assigns_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(value) => {
            expression_reads_name(value, name) || expression_assigns_name(value, name)
        }
        mwcc_syntax_trees::ArmBody::Statements(statements) => statements
            .iter()
            .any(|statement| statement_observes_or_assigns_name(statement, name)),
    }
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

    fn parameter(name: &str) -> Parameter {
        Parameter {
            parameter_type: Type::Int,
            name: name.into(),
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
    fn places_loop_long_parameters_high_and_an_entry_guard_low() {
        let parameters = ["state", "flag", "source", "samples", "destination"]
            .into_iter()
            .map(parameter)
            .collect::<Vec<_>>();
        let locals = vec![local("input"), local("output")];
        let function = Function {
            return_type: Type::Int,
            name: "pressure".into(),
            is_static: false,
            is_weak: false,
            parameters,
            locals,
            statements: vec![
                assign("input", Expression::Variable("source".into())),
                assign("output", Expression::Variable("destination".into())),
                Statement::If {
                    condition: Expression::Variable("flag".into()),
                    then_body: vec![],
                    else_body: vec![],
                },
                Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::Variable("samples".into())),
                    step: None,
                    body: vec![read("input"), read("output")],
                },
                read("state"),
            ],
            guards: vec![],
            return_expression: Some(Expression::Variable("samples".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let ephemeral = function.locals.iter().collect::<Vec<_>>();
        let retained = [3usize, 4, 2, 1, 0]
            .into_iter()
            .map(|index| &function.parameters[index])
            .collect::<Vec<_>>();

        let plan = plan_dense_loop_saved_home_preferences(
            &function,
            &ephemeral,
            Some(12),
            &retained,
            0,
            0,
        );
        assert_eq!(
            (0..5).map(|index| plan.preference(index)).collect::<Vec<_>>(),
            vec![Some(30), Some(31), Some(29), Some(20), Some(28)]
        );
        assert_eq!(plan.forwarded_parameter("input"), Some("source"));
        assert_eq!(
            plan.forwarded_parameter("output"),
            Some("destination")
        );

        let mut source_reused = function.clone();
        source_reused.statements.push(read("source"));
        let ephemeral = source_reused.locals.iter().collect::<Vec<_>>();
        let retained = [3usize, 4, 2, 1, 0]
            .into_iter()
            .map(|index| &source_reused.parameters[index])
            .collect::<Vec<_>>();
        let plan = plan_dense_loop_saved_home_preferences(
            &source_reused,
            &ephemeral,
            Some(12),
            &retained,
            0,
            0,
        );
        assert_eq!(plan.forwarded_parameter("input"), None);
        assert_eq!(
            plan.forwarded_parameter("output"),
            Some("destination")
        );

        let mut target_preassigned = function.clone();
        target_preassigned.statements.insert(
            0,
            assign("input", Expression::Variable("state".into())),
        );
        let ephemeral = target_preassigned.locals.iter().collect::<Vec<_>>();
        let retained = [3usize, 4, 2, 1, 0]
            .into_iter()
            .map(|index| &target_preassigned.parameters[index])
            .collect::<Vec<_>>();
        let plan = plan_dense_loop_saved_home_preferences(
            &target_preassigned,
            &ephemeral,
            Some(12),
            &retained,
            0,
            0,
        );
        assert_eq!(plan.forwarded_parameter("input"), None);
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
