//! Structured control flow whose register values survive conditional calls.
//!
//! This is the conservative bridge between semantic statement lowering and the
//! virtual-register allocator.  It owns a complete function only when every
//! statement is representable by the shared store/call emitter plus forward
//! `if` branches; unsupported control flow declines before emitting anything.

use super::structured_locals::{
    is_definitely_assigned_before_reads, plan_deferred_saved_homes, plan_ephemeral_locals,
};
use super::structured_entry_alias::{plan_first_call_alias, EntryAliasBoundary, EntryParameterAlias};
use super::structured_prologue::saved_home_stores_precede_initialization;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower a void structured body after assigning every value that can be read
    /// following a (possibly conditional) call to a virtual callee-saved home.
    pub(crate) fn try_callee_saved_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !supports_statements(&function.statements, function)
        {
            return Ok(false);
        }

        let candidates: Vec<&str> = function
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .chain(
                function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str()),
            )
            .collect();
        let survivors: std::collections::HashSet<&str> = candidates
            .into_iter()
            .filter(|name| {
                read_after_possible_call(&function.statements, name, false).read_after_call
            })
            .collect();
        // Entry-initialized locals rank ahead of incoming parameters. Deferred
        // locals introduced by nested declarations or inline expansion rank
        // after them and may share a home when their lifetimes do not overlap.
        let saved_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| survivors.contains(local.name.as_str()))
            .collect();
        if saved_locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || class_of(local.declared_type).ok() != Some(ValueClass::General)
                || (local.initializer.is_none()
                    && !is_definitely_assigned_before_reads(&function.statements, &local.name))
        }) {
            return Ok(false);
        }
        let saved_parameters: Vec<_> = function
            .parameters
            .iter()
            .rev()
            .filter(|parameter| survivors.contains(parameter.name.as_str()))
            .collect();
        if saved_parameters.iter().any(|parameter| {
            self.locations
                .get(&parameter.name)
                .is_none_or(|location| location.class != ValueClass::General)
        }) {
            return Ok(false);
        }
        let Some(ephemeral_locals) = plan_ephemeral_locals(function, &survivors) else {
            return Ok(false);
        };
        let (eager_saved_locals, deferred_saved_locals): (Vec<_>, Vec<_>) = saved_locals
            .into_iter()
            .partition(|local| local.initializer.is_some());
        let Some(deferred_home_plan) = plan_deferred_saved_homes(function, &deferred_saved_locals)
        else {
            return Ok(false);
        };

        let count =
            eager_saved_locals.len() + saved_parameters.len() + deferred_home_plan.group_count;
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTable;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -plan.frame_size,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: plan.frame_size + 4,
            },
        ]);

        let batched_saved_home_stores = saved_home_stores_precede_initialization(
            self.behavior.frame_convention,
            eager_saved_locals.len(),
            saved_parameters.len(),
            deferred_home_plan.group_count,
        );

        let saved_parameter_base = eager_saved_locals.len();
        let mut saved_parameter_homes = Vec::with_capacity(saved_parameters.len());
        for (parameter_index, parameter) in saved_parameters.iter().enumerate() {
            let home = homes[saved_parameter_base + parameter_index];
            let incoming = self
                .locations
                .get(&parameter.name)
                .expect("eligibility checked")
                .register;
            saved_parameter_homes.push((parameter.name.clone(), home, incoming));
        }
        let deferred_home_base = saved_parameter_base + saved_parameter_homes.len();
        if batched_saved_home_stores {
            self.emit_structured_saved_home_store_range(
                &homes[..saved_parameter_base],
                0,
                plan.frame_size,
            );
            for (parameter_index, (_, home, incoming)) in
                saved_parameter_homes.iter().enumerate()
            {
                self.emit_structured_saved_home_store(
                    *home,
                    saved_parameter_base + parameter_index,
                    plan.frame_size,
                );
                self.output
                    .instructions
                    .push(Instruction::move_register(*home, *incoming));
            }
            self.emit_structured_saved_home_store_range(
                &homes[deferred_home_base..],
                deferred_home_base,
                plan.frame_size,
            );
        }

        let mut home_index = 0;
        for local in eager_saved_locals {
            let home = homes[home_index];
            home_index += 1;
            if !batched_saved_home_stores {
                self.emit_structured_saved_home_store(home, home_index - 1, plan.frame_size);
            }
            self.evaluate(
                local.initializer.as_ref().expect("partitioned as eager"),
                local.declared_type,
                home,
            )?;
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        for (_, home, incoming) in &saved_parameter_homes {
            home_index += 1;
            if !batched_saved_home_stores {
                self.emit_structured_saved_home_store(*home, home_index - 1, plan.frame_size);
                self.output
                    .instructions
                    .push(Instruction::move_register(*home, *incoming));
            }
        }
        debug_assert_eq!(home_index, deferred_home_base);
        for group in 0..deferred_home_plan.group_count {
            let slot_index = deferred_home_base + group;
            let home = homes[slot_index];
            if !batched_saved_home_stores {
                self.emit_structured_saved_home_store(home, slot_index, plan.frame_size);
            }
        }
        for local in deferred_saved_locals {
            let group = deferred_home_plan.group(&local.name);
            let home = homes[deferred_home_base + group];
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        self.try_preload_ephemeral_float_compare_literal(function, &ephemeral_locals)?;
        // Initializers are evaluated at declaration time, while an incoming
        // parameter still has its entry-register alias. MWCC can preserve that
        // alias after copying the value to a saved home (`mr r31,r3; lwz ...,r3`)
        // and switches subsequent body uses to the home only after declarations.
        for local in &ephemeral_locals {
            let class = class_of(local.declared_type).expect("eligibility checked");
            let temporary = match class {
                ValueClass::General => self.fresh_virtual_general(),
                ValueClass::Float => self.fresh_virtual_float_preferring(
                    self.ephemeral_float_home_preference(function, &ephemeral_locals),
                ),
            };
            if let Some(initializer) = &local.initializer {
                self.evaluate(initializer, local.declared_type, temporary)?;
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
                    register: temporary,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        self.plan_structured_float_handoff(function, &ephemeral_locals);
        let entry_parameter_alias =
            plan_first_call_alias(&function.statements, &saved_parameter_homes);
        for (name, home, _) in saved_parameter_homes {
            if entry_parameter_alias
                .as_ref()
                .is_some_and(|alias| alias.name == name)
            {
                continue;
            }
            self.locations
                .get_mut(&name)
                .expect("eligibility checked")
                .register = home;
        }

        let mut return_branches = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut pending_gotos = Vec::new();
        let statement_start = if entry_parameter_alias
            .as_ref()
            .is_some_and(|alias| alias.boundary == EntryAliasBoundary::AfterFirstStatement)
        {
            let alias = entry_parameter_alias.as_ref().expect("checked above");
            self.emit_structured_statements(
                &function.statements[..1],
                function,
                &mut return_branches,
                &mut label_positions,
                &mut pending_gotos,
                &mut None,
            )?;
            self.locations
                .get_mut(&alias.name)
                .expect("planned saved parameter")
                .register = alias.home;
            1
        } else {
            0
        };
        let mut condition_alias = entry_parameter_alias.filter(|alias| {
            alias.boundary == EntryAliasBoundary::AfterFirstConditionTerm
        });
        self.emit_structured_statements(
            &function.statements[statement_start..],
            function,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut condition_alias,
        )?;
        for (branch, label) in pending_gotos {
            let target = label_positions.get(&label).copied().ok_or_else(|| {
                Diagnostic::error(format!(
                    "structured forward branch targets an unknown label '{label}'"
                ))
            })?;
            if let Instruction::Branch {
                target: branch_target,
            } = &mut self.output.instructions[branch]
            {
                *branch_target = target;
            }
        }
        let epilogue = self.output.instructions.len();
        for branch in return_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        // Each source-level `if` creates a pair of optimizer labels even when
        // both collapse to direct instruction offsets. Build 163 exposes those
        // otherwise-hidden labels through the later unwind-symbol ordinal.
        self.output.anonymous_label_bump += structured_hidden_label_count(&function.statements);
        self.emit_epilogue_and_return();
        self.schedule_legacy_inline_expansion_residue();
        Ok(true)
    }

    fn emit_structured_statements(
        &mut self,
        statements: &[Statement],
        function: &Function,
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        // An early-return guard has no join from its call-making arm. Preserve
        // condition values only along that guard's fallthrough edge, then let
        // the next condition retain the intersection it also reads.
        let mut carried_condition_cache_restore = None;
        for (statement_index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if else_body.is_empty() => {
                    let terms = logical_and_terms(condition);
                    let (previous_cache, previous_float_cache) =
                        if let Some((previous, previous_float)) =
                            carried_condition_cache_restore.take()
                        {
                            self.continue_condition_global_cache(condition);
                            self.continue_condition_float_cache(condition);
                            (previous, previous_float)
                        } else {
                            (
                                self.begin_condition_global_cache(condition),
                                self.begin_condition_float_cache(condition),
                            )
                        };
                    let condition_result = (|| {
                        self.preload_condition_global_cache(condition)?;
                        let mut branches = Vec::with_capacity(terms.len());
                        for (term_index, term) in terms.into_iter().enumerate() {
                            let (options, condition_bit) =
                                self.emit_condition_test(term).map_err(|mut diagnostic| {
                                    diagnostic.message.push_str(&format!(
                                        " (in structured if condition {statement_index})"
                                    ));
                                    diagnostic
                                })?;
                            branches.push(self.output.instructions.len());
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options,
                                    condition_bit,
                                    target: 0,
                                });
                            if statement_index == 0 && term_index == 0 {
                                if let Some(alias) = entry_alias.take() {
                                    self.locations
                                        .get_mut(&alias.name)
                                        .expect("planned saved parameter")
                                        .register = alias.home;
                                }
                            }
                        }
                        Ok(branches)
                    })();
                    let carry_fallthrough_cache =
                        matches!(then_body.last(), Some(Statement::Return(None)))
                            && matches!(
                                statements.get(statement_index + 1),
                                Some(Statement::If { else_body, .. }) if else_body.is_empty()
                            );
                    let continuation_cache = carry_fallthrough_cache.then(|| {
                        (
                            self.condition_global_values.clone(),
                            self.condition_float_cache.clone(),
                        )
                    });
                    self.restore_condition_global_cache(previous_cache);
                    self.restore_condition_float_cache(previous_float_cache);
                    let branches = condition_result?;
                    self.commit_structured_float_handoff();
                    self.emit_structured_statements(
                        then_body,
                        function,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )
                    .map_err(|mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (inside structured if statement {statement_index})"
                        ));
                        diagnostic
                    })?;
                    let target = self.output.instructions.len();
                    for branch in branches {
                        if let Instruction::BranchConditionalForward {
                            target: branch_target,
                            ..
                        } = &mut self.output.instructions[branch]
                        {
                            *branch_target = target;
                        }
                    }
                    if let Some((cache, float_cache)) = continuation_cache {
                        let previous = std::mem::replace(&mut self.condition_global_values, cache);
                        let previous_float =
                            std::mem::replace(&mut self.condition_float_cache, float_cache);
                        carried_condition_cache_restore = Some((previous, previous_float));
                    }
                }
                Statement::Return(None) => {
                    return_branches.push(self.output.instructions.len());
                    self.output
                        .instructions
                        .push(Instruction::Branch { target: 0 });
                }
                Statement::Goto(label) => {
                    let branch = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::Branch { target: 0 });
                    pending_gotos.push((branch, label.clone()));
                }
                Statement::Label(label) => {
                    if label_positions
                        .insert(label.clone(), self.output.instructions.len())
                        .is_some()
                    {
                        return Err(Diagnostic::error(format!(
                            "structured body defines label '{label}' more than once"
                        )));
                    }
                }
                Statement::Assign { name, value } => {
                    let local = function
                        .locals
                        .iter()
                        .find(|local| &local.name == name)
                        .expect("eligibility checked");
                    let destination = self
                        .locations
                        .get(name)
                        .ok_or_else(|| {
                            Diagnostic::error("structured assignment has no register home")
                        })?
                        .register;
                    self.evaluate(value, local.declared_type, destination)
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (in structured assignment statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                }
                _ => self.emit_statement(statement).map_err(|mut diagnostic| {
                    diagnostic.message.push_str(&format!(
                        " (in structured body statement {statement_index})"
                    ));
                    diagnostic
                })?,
            }
        }
        if let Some((previous, previous_float)) = carried_condition_cache_restore {
            self.restore_condition_global_cache(previous);
            self.restore_condition_float_cache(previous_float);
        }
        Ok(())
    }
}

fn supports_statements(statements: &[Statement], function: &Function) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. }
        | Statement::Expression(_)
        | Statement::Return(None)
        | Statement::Goto(_)
        | Statement::Label(_) => true,
        Statement::Assign { name, .. } => function.locals.iter().any(|local| &local.name == name),
        Statement::If {
            then_body,
            else_body,
            ..
        } => else_body.is_empty() && supports_statements(then_body, function),
        _ => false,
    })
}

fn structured_hidden_label_count(statements: &[Statement]) -> u32 {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                2 + logical_and_count(condition)
                    + structured_hidden_label_count(then_body)
                    + structured_hidden_label_count(else_body)
            }
            _ => 0,
        })
        .sum()
}

fn logical_and_count(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } => 1 + logical_and_count(left) + logical_and_count(right),
        _ => 0,
    }
}

pub(super) fn logical_and_terms(expression: &Expression) -> Vec<&Expression> {
    let mut terms = Vec::new();
    fn collect<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
        if let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } = expression
        {
            collect(left, terms);
            collect(right, terms);
        } else {
            terms.push(expression);
        }
    }
    collect(expression, &mut terms);
    terms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flow {
    read_after_call: bool,
    call_on_fallthrough: bool,
    falls_through: bool,
}

/// Path-sensitive call liveness for one structured statement sequence. A call
/// in an arm that returns does not contaminate the continuation, while a call in
/// the condition reaches either arm and can make their reads require a saved
/// home.
fn read_after_possible_call(statements: &[Statement], name: &str, mut prior_call: bool) -> Flow {
    let mut read_after = false;
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                read_after |= prior_call && expression_reads_name(condition, name);
                let branch_entry_call = prior_call || expression_has_call(condition);
                let then_flow = read_after_possible_call(then_body, name, branch_entry_call);
                let else_flow = read_after_possible_call(else_body, name, branch_entry_call);
                read_after |= then_flow.read_after_call || else_flow.read_after_call;
                let then_reaches = then_flow
                    .falls_through
                    .then_some(then_flow.call_on_fallthrough);
                let else_reaches = else_flow
                    .falls_through
                    .then_some(else_flow.call_on_fallthrough);
                match (then_reaches, else_reaches) {
                    (None, None) => {
                        return Flow {
                            read_after_call: read_after,
                            call_on_fallthrough: false,
                            falls_through: false,
                        };
                    }
                    (then_call, else_call) => {
                        prior_call = then_call.unwrap_or(false) || else_call.unwrap_or(false);
                    }
                }
            }
            Statement::Store { target, value } => {
                read_after |= prior_call
                    && (expression_reads_name(target, name) || expression_reads_name(value, name));
                prior_call |= statement_has_call(statement);
            }
            Statement::Assign {
                name: assigned_name,
                value,
            } => {
                read_after |= prior_call && expression_reads_name(value, name);
                if assigned_name == name {
                    // A fresh definition after a call kills the old value's
                    // cross-call lifetime. A self-referential update retains it.
                    prior_call = expression_has_call(value)
                        || (prior_call && expression_reads_name(value, name));
                } else {
                    prior_call |= statement_has_call(statement);
                }
            }
            Statement::Expression(value) => {
                read_after |= prior_call && expression_reads_name(value, name);
                prior_call |= statement_has_call(statement);
            }
            Statement::Return(expression) => {
                read_after |= prior_call
                    && expression
                        .as_ref()
                        .is_some_and(|value| expression_reads_name(value, name));
                return Flow {
                    read_after_call: read_after,
                    call_on_fallthrough: false,
                    falls_through: false,
                };
            }
            Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_)
            | Statement::Switch { .. }
            | Statement::Loop { .. } => {
                prior_call |= statement_has_call(statement);
            }
        }
    }
    Flow {
        read_after_call: read_after,
        call_on_fallthrough: prior_call,
        falls_through: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_calls_make_later_reads_survive() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "grow".into(),
                    arguments: vec![],
                })],
                else_body: vec![],
            },
            Statement::Store {
                target: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("pointer".into())),
                },
                value: Expression::IntegerLiteral(1),
            },
        ];
        assert!(read_after_possible_call(&statements, "pointer", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "condition", false).read_after_call);
    }

    #[test]
    fn a_calling_arm_that_returns_does_not_reach_the_continuation() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![
                    Statement::Expression(Expression::Call {
                        name: "act".into(),
                        arguments: vec![],
                    }),
                    Statement::Return(None),
                ],
                else_body: vec![],
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_makes_reads_in_its_arm_live_across_the_call() {
        let statements = vec![Statement::If {
            condition: Expression::Call {
                name: "test".into(),
                arguments: vec![],
            },
            then_body: vec![Statement::Expression(Expression::Variable("value".into()))],
            else_body: vec![],
        }];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_fresh_assignment_kills_an_earlier_call_lifetime() {
        let statements = vec![
            Statement::Expression(Expression::Call {
                name: "before".into(),
                arguments: vec![],
            }),
            Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);

        let self_update = vec![
            Statement::Expression(Expression::Call {
                name: "before".into(),
                arguments: vec![],
            }),
            Statement::Assign {
                name: "value".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("value".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            },
        ];
        assert!(read_after_possible_call(&self_update, "value", false).read_after_call);
    }
}
