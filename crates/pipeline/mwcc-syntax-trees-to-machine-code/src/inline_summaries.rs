//! Small, semantics-first summaries of function bodies that mwcc may inline.
//!
//! Call-site schedules must not infer an implementation from a callee's name.
//! This module recognizes exact helper bodies once per translation unit and
//! exposes only the facts an inlining composition needs.  Keeping the source
//! AST out of `Generator` also avoids turning instruction selection into an
//! ad-hoc interprocedural analyzer as more inline families are added.

use crate::analysis::constant_value;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement, Type};
use std::collections::{HashMap, HashSet};

use crate::body::{
    function_calls_any, summarize_queue_pop, summarize_queue_service, QueuePopSummary,
    QueueServiceSummary,
};

#[derive(Clone, Debug)]
pub(crate) struct FixedPollSummary {
    pub(crate) bank: String,
    pub(crate) index: i64,
    pub(crate) mask: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct FixedLocalRmwSummary {
    pub(crate) bank: String,
    pub(crate) index: i64,
    pub(crate) preserve_mask: i16,
    pub(crate) set_bits: u16,
}

/// Verified helper-body facts available while lowering one translation unit.
#[derive(Clone, Debug, Default)]
pub struct InlineSummaries {
    fixed_polls: HashMap<String, FixedPollSummary>,
    fixed_local_rmws: HashMap<String, FixedLocalRmwSummary>,
    queue_pops: HashMap<String, QueuePopSummary>,
    queue_services: HashMap<String, QueueServiceSummary>,
    queue_services_with_callers: HashSet<String>,
}

impl InlineSummaries {
    /// Analyze every definition once. A function is recorded only when its
    /// entire body has the summarized semantics; near misses remain ordinary
    /// calls and cannot accidentally claim an inline-only caller schedule.
    pub fn analyze(functions: &[Function]) -> Self {
        let mut summaries = Self::default();
        for function in functions {
            if let Some(summary) = summarize_fixed_poll(function) {
                summaries.fixed_polls.insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_fixed_local_rmw(function) {
                summaries
                    .fixed_local_rmws
                    .insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_queue_pop(function) {
                summaries.queue_pops.insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_queue_service(function) {
                summaries
                    .queue_services
                    .insert(function.name.clone(), summary);
            }
        }
        // Build 163's label walk gives a summarized service helper three fewer
        // private ordinals when another definition calls it. The helper is an
        // inline candidate even where this generation ultimately leaves the
        // service call out of line.
        for name in summaries.queue_services.keys() {
            let singleton = HashSet::from([name.clone()]);
            if functions
                .iter()
                .any(|function| function.name != *name && function_calls_any(function, &singleton))
            {
                summaries.queue_services_with_callers.insert(name.clone());
            }
        }
        summaries
    }

    pub(crate) fn fixed_poll(&self, name: &str) -> Option<&FixedPollSummary> {
        self.fixed_polls.get(name)
    }

    pub(crate) fn fixed_local_rmw(&self, name: &str) -> Option<&FixedLocalRmwSummary> {
        self.fixed_local_rmws.get(name)
    }

    pub(crate) fn queue_pop(&self, name: &str) -> Option<&QueuePopSummary> {
        self.queue_pops.get(name)
    }

    pub(crate) fn queue_service(&self, name: &str) -> Option<&QueueServiceSummary> {
        self.queue_services.get(name)
    }

    pub(crate) fn queue_service_has_caller(&self, name: &str) -> bool {
        self.queue_services_with_callers.contains(name)
    }
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn fixed_slot(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Index { base, index } = peel_casts(expression) else {
        return None;
    };
    let Expression::Variable(bank) = base.as_ref() else {
        return None;
    };
    Some((bank, constant_value(index)?))
}

fn is_plain_void_helper(function: &Function) -> bool {
    function.return_type == Type::Void
        && function.parameters.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
}

fn summarize_fixed_poll(function: &Function) -> Option<FixedPollSummary> {
    if !is_plain_void_helper(function) || !function.locals.is_empty() {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = peel_casts(condition)
    else {
        return None;
    };
    let (access, mask) = if let Some(mask) = constant_value(right) {
        (left.as_ref(), mask)
    } else {
        (right.as_ref(), constant_value(left)?)
    };
    let mask = u32::try_from(mask).ok().filter(|mask| *mask != 0)?;
    let (bank, index) = fixed_slot(access)?;
    Some(FixedPollSummary {
        bank: bank.to_string(),
        index,
        mask,
    })
}

fn summarize_fixed_local_rmw(function: &Function) -> Option<FixedLocalRmwSummary> {
    if !is_plain_void_helper(function) || function.locals.len() != 1 {
        return None;
    }
    let [temporary] = function.locals.as_slice() else {
        return None;
    };
    if temporary.declared_type != Type::UnsignedShort
        || temporary.array_length.is_some()
        || temporary.is_static
        || temporary.initializer.is_some()
    {
        return None;
    }
    let [Statement::Assign {
        name: loaded_name,
        value: loaded_value,
    }, Statement::Assign {
        name: updated_name,
        value: updated_value,
    }, Statement::Store {
        target,
        value: stored_value,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if loaded_name != &temporary.name
        || updated_name != &temporary.name
        || !matches!(stored_value, Expression::Variable(name) if name == &temporary.name)
    {
        return None;
    }
    let (bank, index) = fixed_slot(loaded_value)?;
    let (stored_bank, stored_index) = fixed_slot(target)?;
    if bank != stored_bank || index != stored_index {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: preserved,
        right: set_bits,
    } = peel_casts(updated_value)
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: preserved_value,
        right: preserve_mask,
    } = peel_casts(preserved)
    else {
        return None;
    };
    if !matches!(peel_casts(preserved_value), Expression::Variable(name) if name == &temporary.name)
    {
        return None;
    }
    Some(FixedLocalRmwSummary {
        bank: bank.to_string(),
        index,
        preserve_mask: i16::try_from(constant_value(preserve_mask)?).ok()?,
        set_bits: u16::try_from(constant_value(set_bits)?)
            .ok()
            .filter(|bits| *bits != 0)?,
    })
}
