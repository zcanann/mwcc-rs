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
    function_calls_any, summarize_queue_pop, summarize_queue_service,
    summarize_unoptimized_local_select, QueuePopSummary, QueueServiceSummary,
    UnoptimizedLocalSelectSummary,
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

/// A parameterless static helper whose entire body is one direct void call.
/// mwcc may retain the helper's out-of-line body while also expanding this call
/// at a caller that owns a larger schedule (for example a REL ctor walker).
#[derive(Clone, Debug)]
pub(crate) struct StaticCallWrapperSummary {
    pub(crate) callee: String,
    pub(crate) arguments: Vec<Expression>,
}

/// A two-pointer wrapper whose entire body forwards to a fixed-size byte copy.
#[derive(Clone, Debug)]
pub(crate) struct FixedSizeCopySummary {
    pub(crate) callee: String,
    pub(crate) byte_count: i16,
}

/// An empty deleting destructor whose only lifetime action is destroying one
/// non-virtual base. Under file IPA, GC 4.1 may inline this wrapper into a
/// derived destructor while retaining the wrapper's null guard.
#[derive(Clone, Debug)]
pub(crate) struct SingleBaseDestructorSummary {
    pub(crate) callee: String,
    pub(crate) adjustment: u32,
}

/// A NULL-terminated function-pointer table walker eligible for whole-file
/// inlining. The loop body is summarized to its only externally visible input:
/// the table symbol whose entries it invokes.
#[derive(Clone, Debug)]
pub(crate) struct PointerWalkerSummary {
    pub(crate) table: String,
}

/// A byte append helper that bounds a cursor, stores one byte with post-step,
/// raises the logical length, and returns success/overflow status.
#[derive(Clone, Debug)]
pub(crate) struct ByteAppendSummary {
    pub(crate) capacity: u16,
    pub(crate) overflow: i16,
    pub(crate) length_offset: i16,
    pub(crate) position_offset: i16,
    pub(crate) data_offset: i16,
}

/// Verified helper-body facts available while lowering one translation unit.
#[derive(Clone, Debug, Default)]
pub struct InlineSummaries {
    fixed_polls: HashMap<String, FixedPollSummary>,
    fixed_local_rmws: HashMap<String, FixedLocalRmwSummary>,
    queue_pops: HashMap<String, QueuePopSummary>,
    queue_services: HashMap<String, QueueServiceSummary>,
    queue_services_with_callers: HashSet<String>,
    static_call_wrappers: HashMap<String, StaticCallWrapperSummary>,
    ipa_call_wrappers: HashMap<String, StaticCallWrapperSummary>,
    pointer_walkers: HashMap<String, PointerWalkerSummary>,
    unoptimized_local_selects: HashMap<String, UnoptimizedLocalSelectSummary>,
    byte_appends: HashMap<String, ByteAppendSummary>,
    fixed_size_copies: HashMap<String, FixedSizeCopySummary>,
    single_base_destructors: HashMap<String, SingleBaseDestructorSummary>,
    ipa_elided_functions: HashSet<String>,
}

impl InlineSummaries {
    /// Analyze every definition once. A function is recorded only when its
    /// entire body has the summarized semantics; near misses remain ordinary
    /// calls and cannot accidentally claim an inline-only caller schedule.
    pub fn analyze(functions: &[Function]) -> Self {
        Self::analyze_with_skipped(functions, &[])
    }

    /// Analyze emitted definitions together with analysis-only skipped inline
    /// definitions. The latter can prove call-site expansion semantics but are
    /// never candidates for object emission or whole-file elision.
    pub fn analyze_with_skipped(functions: &[Function], skipped: &[Function]) -> Self {
        let mut summaries = Self::default();
        let definitions: Vec<&Function> = functions.iter().chain(skipped).collect();
        for function in &definitions {
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
            if let Some(summary) = summarize_static_call_wrapper(function) {
                summaries
                    .static_call_wrappers
                    .insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_fixed_size_copy(function) {
                summaries
                    .fixed_size_copies
                    .insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_single_base_destructor(function) {
                summaries
                    .single_base_destructors
                    .insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_call_wrapper(function, false) {
                summaries
                    .ipa_call_wrappers
                    .insert(function.name.clone(), summary);
            }
            if let Some(summary) = summarize_pointer_walker(function) {
                summaries
                    .pointer_walkers
                    .insert(function.name.clone(), summary);
            }
            if function.is_static {
                if let Some(summary) = summarize_unoptimized_local_select(function) {
                    summaries
                        .unoptimized_local_selects
                        .insert(function.name.clone(), summary);
                }
            }
            if let Some(summary) = summarize_byte_append(function) {
                summaries
                    .byte_appends
                    .insert(function.name.clone(), summary);
            }
        }
        // Build 163's label walk gives a summarized service helper three fewer
        // private ordinals when another definition calls it. The helper is an
        // inline candidate even where this generation ultimately leaves the
        // service call out of line.
        for name in summaries.queue_services.keys() {
            let singleton = HashSet::from([name.clone()]);
            if definitions
                .iter()
                .any(|function| function.name != *name && function_calls_any(function, &singleton))
            {
                summaries.queue_services_with_callers.insert(name.clone());
            }
        }
        for walker in functions.iter().filter(|function| {
            function.is_static && summaries.pointer_walkers.contains_key(&function.name)
        }) {
            let singleton = HashSet::from([walker.name.clone()]);
            let callers: Vec<_> = functions
                .iter()
                .filter(|function| function_calls_any(function, &singleton))
                .collect();
            if callers.len() == 1 && summaries.ipa_pointer_walker_caller(callers[0]).is_some() {
                summaries.ipa_elided_functions.insert(walker.name.clone());
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

    pub(crate) fn static_call_wrapper(&self, name: &str) -> Option<&StaticCallWrapperSummary> {
        self.static_call_wrappers.get(name)
    }

    pub(crate) fn fixed_size_copy(&self, name: &str) -> Option<&FixedSizeCopySummary> {
        self.fixed_size_copies.get(name)
    }

    pub(crate) fn single_base_destructor(
        &self,
        name: &str,
    ) -> Option<&SingleBaseDestructorSummary> {
        self.single_base_destructors.get(name)
    }

    pub(crate) fn pointer_walker(&self, name: &str) -> Option<&PointerWalkerSummary> {
        self.pointer_walkers.get(name)
    }

    pub(crate) fn unoptimized_local_select(
        &self,
        name: &str,
    ) -> Option<&UnoptimizedLocalSelectSummary> {
        self.unoptimized_local_selects.get(name)
    }

    pub(crate) fn byte_append(&self, name: &str) -> Option<&ByteAppendSummary> {
        self.byte_appends.get(name)
    }

    pub(crate) fn ipa_pointer_walker_caller(
        &self,
        function: &Function,
    ) -> Option<(&PointerWalkerSummary, Option<&StaticCallWrapperSummary>)> {
        if function.return_type != Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return None;
        }
        let calls: Vec<_> = function
            .statements
            .iter()
            .map(|statement| match statement {
                Statement::Expression(Expression::Call { name, arguments })
                    if arguments.is_empty() =>
                {
                    Some(name.as_str())
                }
                _ => None,
            })
            .collect::<Option<_>>()?;
        let walker = self.pointer_walker(calls.first()?)?;
        let trailing = match calls.as_slice() {
            [_] => None,
            [_, wrapper] => Some(self.ipa_call_wrappers.get(*wrapper)?),
            _ => return None,
        };
        Some((walker, trailing))
    }

    pub fn should_elide_ipa_function(&self, name: &str) -> bool {
        self.ipa_elided_functions.contains(name)
    }
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn summarize_single_base_destructor(function: &Function) -> Option<SingleBaseDestructorSummary> {
    if !function.name.starts_with("__dt__")
        || !matches!(function.return_type, Type::StructPointer { .. })
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || function.parameters[1].name != "__destroy"
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || function.parameters[1].parameter_type != Type::Short
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !function
            .return_expression
            .as_ref()
            .is_some_and(|expression| variable(expression, "this"))
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !variable(condition, "this") || !else_body.is_empty() {
        return None;
    }
    let [base_call, delete_guard] = then_body.as_slice() else {
        return None;
    };
    let Statement::Expression(Expression::Call { name, arguments }) = base_call else {
        return None;
    };
    let [object, Expression::IntegerLiteral(0)] = arguments.as_slice() else {
        return None;
    };
    let adjustment = match object {
        Expression::Variable(name) if name == "this" => 0,
        Expression::MemberAddress { base, offset, .. }
            if variable(base, "this") => *offset,
        _ => return None,
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body: delete_body,
        else_body: delete_else,
    } = delete_guard
    else {
        return None;
    };
    if !variable(left, "__destroy")
        || constant_value(right) != Some(0)
        || delete_body.len() != 1
        || !delete_else.is_empty()
    {
        return None;
    }
    Some(SingleBaseDestructorSummary {
        callee: name.clone(),
        adjustment,
    })
}

fn struct_member(expression: &Expression, base: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base: found,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(found, base).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn summarize_byte_append(function: &Function) -> Option<ByteAppendSummary> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [buffer, byte] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || byte.parameter_type != Type::UnsignedChar
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }, byte_store, length_store] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let capacity = match condition {
        Expression::Binary {
            operator: BinaryOperator::GreaterEqual,
            left,
            right,
        } => {
            let (_, member_type) = struct_member(left, &buffer.name)?;
            if member_type != Type::UnsignedInt {
                return None;
            }
            u16::try_from(constant_value(right)?).ok()?
        }
        _ => return None,
    };
    let position_offset = match condition {
        Expression::Binary { left, .. } => struct_member(left, &buffer.name)?.0,
        _ => unreachable!(),
    };
    let [Statement::Return(Some(overflow))] = then_body.as_slice() else {
        return None;
    };
    let overflow = i16::try_from(constant_value(overflow)?).ok()?;

    let Statement::Store {
        target: Expression::Index { base, index },
        value,
    } = byte_store
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: data_buffer,
        offset: data_offset,
        element: mwcc_syntax_trees::Pointee::UnsignedChar,
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    if !variable(data_buffer, &buffer.name)
        || !variable(value, &byte.name)
        || !matches!(index.as_ref(), Expression::PostStep {
            target,
            operator: BinaryOperator::Add,
        } if struct_member(target, &buffer.name)
            == Some((position_offset, Type::UnsignedInt)))
    {
        return None;
    }
    let data_offset = i16::try_from(*data_offset).ok()?;

    let Statement::Store { target, value } = length_store else {
        return None;
    };
    let (length_offset, length_type) = struct_member(target, &buffer.name)?;
    if length_type != Type::UnsignedInt
        || !matches!(value, Expression::Binary {
            operator: BinaryOperator::Add, left, right
        } if struct_member(left, &buffer.name) == Some((length_offset, Type::UnsignedInt))
            && constant_value(right) == Some(1))
    {
        return None;
    }
    Some(ByteAppendSummary {
        capacity,
        overflow,
        length_offset,
        position_offset,
        data_offset,
    })
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

fn summarize_static_call_wrapper(function: &Function) -> Option<StaticCallWrapperSummary> {
    summarize_call_wrapper(function, true)
}

fn summarize_fixed_size_copy(function: &Function) -> Option<FixedSizeCopySummary> {
    if function.return_type != Type::Void
        || function.parameters.len() != 2
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || !function.parameters.iter().all(|parameter| {
            matches!(
                parameter.parameter_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
        })
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] =
        function.statements.as_slice()
    else {
        return None;
    };
    let [Expression::Variable(first), Expression::Variable(second), byte_count] =
        arguments.as_slice()
    else {
        return None;
    };
    if first != &function.parameters[0].name || second != &function.parameters[1].name {
        return None;
    }
    Some(FixedSizeCopySummary {
        callee: name.clone(),
        byte_count: i16::try_from(constant_value(byte_count)?).ok()?,
    })
}

fn summarize_call_wrapper(
    function: &Function,
    require_static: bool,
) -> Option<StaticCallWrapperSummary> {
    if (require_static && !function.is_static)
        || !is_plain_void_helper(function)
        || !function.locals.is_empty()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] =
        function.statements.as_slice()
    else {
        return None;
    };
    if name == &function.name {
        return None;
    }
    Some(StaticCallWrapperSummary {
        callee: name.clone(),
        arguments: arguments.clone(),
    })
}

fn summarize_pointer_walker(function: &Function) -> Option<PointerWalkerSummary> {
    if !is_plain_void_helper(function) || !function.parameters.is_empty() {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if local.initializer.is_some() || local.array_length.is_some() || local.is_static {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Assign { target, value }),
        condition: Some(Expression::Dereference { pointer }),
        step:
            Some(Expression::Assign {
                target: step_target,
                value: step_value,
            }),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(target.as_ref(), Expression::Variable(name) if name == &local.name)
        || !matches!(pointer.as_ref(), Expression::Variable(name) if name == &local.name)
        || !matches!(step_target.as_ref(), Expression::Variable(name) if name == &local.name)
        || !matches!(step_value.as_ref(), Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == &local.name)
            && matches!(right.as_ref(), Expression::IntegerLiteral(1)))
        || !matches!(body.as_slice(), [Statement::Expression(Expression::Call { name, arguments })]
            if name == &local.name && arguments.is_empty())
    {
        return None;
    }
    let Expression::Variable(table) = value.as_ref() else {
        return None;
    };
    Some(PointerWalkerSummary {
        table: table.clone(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter, Pointee};

    fn wrapper(name: &str, is_static: bool, statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: name.into(),
            is_static,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn pointer_walker(name: &str, is_static: bool, table: &str) -> Function {
        let local = "entry".to_string();
        let mut function = wrapper(name, is_static, Vec::new());
        function.locals.push(LocalDeclaration {
            declared_type: Type::Pointer(Pointee::Pointer),
            name: local.clone(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        function.statements.push(Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable(local.clone())),
                value: Box::new(Expression::Variable(table.into())),
            }),
            condition: Some(Expression::Dereference {
                pointer: Box::new(Expression::Variable(local.clone())),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable(local.clone())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable(local.clone())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body: vec![Statement::Expression(Expression::Call {
                name: local,
                arguments: Vec::new(),
            })],
        });
        function
    }

    #[test]
    fn static_call_wrapper_requires_the_entire_helper_shape() {
        let direct_call = Statement::Expression(Expression::Call {
            name: "target".into(),
            arguments: vec![Expression::IntegerLiteral(7)],
        });
        let valid = wrapper("helper", true, vec![direct_call.clone()]);
        let summaries = InlineSummaries::analyze(&[valid]);
        let summary = summaries.static_call_wrapper("helper").unwrap();
        assert_eq!(summary.callee, "target");
        assert!(matches!(
            summary.arguments.as_slice(),
            [Expression::IntegerLiteral(7)]
        ));

        let external = wrapper("external", false, vec![direct_call]);
        let recursive = wrapper(
            "recursive",
            true,
            vec![Statement::Expression(Expression::Call {
                name: "recursive".into(),
                arguments: Vec::new(),
            })],
        );
        let extra_statement = wrapper(
            "extra",
            true,
            vec![
                Statement::Expression(Expression::Call {
                    name: "target".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::IntegerLiteral(0)),
            ],
        );
        let near_misses = InlineSummaries::analyze(&[external, recursive, extra_statement]);
        assert!(near_misses.static_call_wrapper("external").is_none());
        assert!(near_misses.static_call_wrapper("recursive").is_none());
        assert!(near_misses.static_call_wrapper("extra").is_none());
    }

    #[test]
    fn fixed_size_copy_requires_two_forwarded_pointer_parameters() {
        let mut function = wrapper(
            "copy_record",
            false,
            vec![Statement::Expression(Expression::Call {
                name: "copy_bytes".into(),
                arguments: vec![
                    Expression::Variable("destination".into()),
                    Expression::Variable("source".into()),
                    Expression::IntegerLiteral(12),
                ],
            })],
        );
        function.parameters = vec![
            Parameter {
                parameter_type: Type::Pointer(Pointee::Char),
                name: "destination".into(),
            },
            Parameter {
                parameter_type: Type::Pointer(Pointee::Char),
                name: "source".into(),
            },
        ];

        let summaries = InlineSummaries::analyze(&[function]);
        let copy = summaries.fixed_size_copy("copy_record").unwrap();
        assert_eq!(copy.callee, "copy_bytes");
        assert_eq!(copy.byte_count, 12);
    }

    #[test]
    fn single_base_destructor_requires_the_complete_empty_wrapper_shape() {
        let base_call = Statement::Expression(Expression::Call {
            name: "__dt__4BaseFv".into(),
            arguments: vec![
                Expression::Variable("this".into()),
                Expression::IntegerLiteral(0),
            ],
        });
        let delete_guard = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Greater,
                left: Box::new(Expression::Variable("__destroy".into())),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Expression(Expression::Call {
                name: "__dl__FPv".into(),
                arguments: vec![Expression::Variable("this".into())],
            })],
            else_body: Vec::new(),
        };
        let mut destructor = wrapper(
            "__dt__7DerivedFv",
            false,
            vec![Statement::If {
                condition: Expression::Variable("this".into()),
                then_body: vec![base_call, delete_guard],
                else_body: Vec::new(),
            }],
        );
        destructor.return_type = Type::StructPointer { element_size: 8 };
        destructor.parameters = vec![
            Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "this".into(),
            },
            Parameter {
                parameter_type: Type::Short,
                name: "__destroy".into(),
            },
        ];
        destructor.return_expression = Some(Expression::Variable("this".into()));

        let summaries = InlineSummaries::analyze(&[destructor]);
        let summary = summaries
            .single_base_destructor("__dt__7DerivedFv")
            .expect("the exact empty wrapper should be summarized");
        assert_eq!(summary.callee, "__dt__4BaseFv");
        assert_eq!(summary.adjustment, 0);
    }

    #[test]
    fn ipa_pointer_walker_composes_verified_callers_and_elides_one_static_callee() {
        let init_walker = pointer_walker("__init_cpp", false, "_ctors");
        let init_user = wrapper(
            "__init_user",
            false,
            vec![Statement::Expression(Expression::Call {
                name: "__init_cpp".into(),
                arguments: Vec::new(),
            })],
        );
        let fini_walker = pointer_walker("__fini_cpp", true, "_dtors");
        let mut exit = wrapper(
            "exit",
            false,
            vec![
                Statement::Expression(Expression::Call {
                    name: "__fini_cpp".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "_ExitProcess".into(),
                    arguments: Vec::new(),
                }),
            ],
        );
        exit.parameters.push(Parameter {
            parameter_type: Type::Int,
            name: "status".into(),
        });
        let exit_process = wrapper(
            "_ExitProcess",
            false,
            vec![Statement::Expression(Expression::Call {
                name: "PPCHalt".into(),
                arguments: Vec::new(),
            })],
        );

        let summaries = InlineSummaries::analyze(&[
            init_user.clone(),
            init_walker,
            fini_walker,
            exit.clone(),
            exit_process,
        ]);
        assert_eq!(
            summaries.pointer_walker("__init_cpp").unwrap().table,
            "_ctors"
        );
        assert_eq!(
            summaries
                .ipa_pointer_walker_caller(&init_user)
                .unwrap()
                .0
                .table,
            "_ctors"
        );
        let (walker, trailing) = summaries.ipa_pointer_walker_caller(&exit).unwrap();
        assert_eq!(walker.table, "_dtors");
        assert_eq!(trailing.unwrap().callee, "PPCHalt");
        assert!(summaries.should_elide_ipa_function("__fini_cpp"));
        assert!(!summaries.should_elide_ipa_function("__init_cpp"));
    }
}
