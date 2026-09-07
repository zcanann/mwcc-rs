//! Boolean call-result accumulation in structured bodies.
//!
//! Source such as `error |= !operation()` is one cross-call recurrence. The
//! general expression evaluator deliberately rejects nested calls, so this
//! owner exposes the recurrence explicitly: call, normalize the zero result to
//! one bit, then merge it with the callee-saved accumulator.

#[allow(unused_imports)]
use super::*;

pub(super) fn call_accumulator_names(function: &Function) -> std::collections::HashSet<&str> {
    let mut names = std::collections::HashSet::new();
    collect_call_accumulator_names(&function.statements, &mut names);
    names.retain(|name| accumulator_value_is_observed(function, name));
    names
}

pub(super) fn accumulator_value_is_observed(function: &Function, name: &str) -> bool {
    function
        .return_expression
        .as_ref()
        .is_some_and(|value| expression_reads_name(value, name))
        || function.guards.iter().any(|guard| {
            expression_reads_name(&guard.condition, name)
                || expression_reads_name(&guard.value, name)
        })
        || statements_observe_accumulator(&function.statements, name)
}

fn statements_observe_accumulator(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign {
            name: assigned,
            value,
        } if assigned == name && is_call_accumulator_value(name, value) => false,
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_reads_name(value, name)
        }
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
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
                || statements_observe_accumulator(body, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || statements_observe_accumulator(then_body, name)
                || statements_observe_accumulator(else_body, name)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_reads_name(scrutinee, name)
                || arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(value) => {
                        expression_reads_name(value, name)
                    }
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        statements_observe_accumulator(body, name)
                    }
                })
                || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(value) => {
                        expression_reads_name(value, name)
                    }
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        statements_observe_accumulator(body, name)
                    }
                })
        }
        Statement::Return(value) => value
            .as_ref()
            .is_some_and(|value| expression_reads_name(value, name)),
        Statement::InlineAsm(_) => true,
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
            false
        }
    })
}

fn collect_call_accumulator_names<'a>(
    statements: &'a [Statement],
    names: &mut std::collections::HashSet<&'a str>,
) {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } => {
                if is_call_accumulator_value(name, value) {
                    names.insert(name);
                }
            }
            Statement::Loop { body, .. } => {
                collect_call_accumulator_names(body, names);
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_call_accumulator_names(then_body, names);
                collect_call_accumulator_names(else_body, names);
            }
            Statement::Switch { arms, default, .. } => {
                for arm in arms {
                    if let mwcc_syntax_trees::ArmBody::Statements(body) = &arm.body {
                        collect_call_accumulator_names(body, names);
                    }
                }
                if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                    collect_call_accumulator_names(body, names);
                }
            }
            _ => {}
        }
    }
}

pub(super) fn call_accumulator_assignment_count(function: &Function) -> u32 {
    count_call_accumulator_assignments(&function.statements)
}

fn count_call_accumulator_assignments(statements: &[Statement]) -> u32 {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Assign { name, value } => u32::from(is_call_accumulator_value(name, value)),
            Statement::Loop { body, .. } => count_call_accumulator_assignments(body),
            Statement::If {
                then_body,
                else_body,
                ..
            } => count_call_accumulator_assignments(then_body)
                .saturating_add(count_call_accumulator_assignments(else_body)),
            Statement::Switch { arms, default, .. } => arms
                .iter()
                .filter_map(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Statements(body) => Some(body.as_slice()),
                    mwcc_syntax_trees::ArmBody::Return(_) => None,
                })
                .chain(default.iter().filter_map(|body| match body {
                    mwcc_syntax_trees::ArmBody::Statements(body) => Some(body.as_slice()),
                    mwcc_syntax_trees::ArmBody::Return(_) => None,
                }))
                .map(count_call_accumulator_assignments)
                .fold(0, u32::saturating_add),
            _ => 0,
        })
        .fold(0, u32::saturating_add)
}

pub(super) fn in_place_call_combined_return_name(function: &Function) -> Option<&str> {
    let Expression::Variable(returned) = function.return_expression.as_ref()? else {
        return None;
    };
    function.statements.iter().any(|statement| {
        matches!(
            statement,
            Statement::Assign {
                name,
                value: Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left,
                    right,
                },
            } if name == returned
                && matches!(left.as_ref(), Expression::Variable(read) if read == name)
                && matches!(right.as_ref(), Expression::Call { .. })
        )
    }).then_some(returned.as_str())
}

/// Replace the first `error |= !call()` with `error = !call()` when the
/// declaration's zero reaches it unchanged. The straight-line prefix cannot
/// contain an entry label, so later control flow cannot revisit the folded
/// update. Reads include address-taking; an escaped local is never assumed zero.
pub(super) fn fold_entry_zero_call_accumulators(function: &Function) -> Option<Function> {
    fn mentions(value: &Expression, name: &str) -> bool {
        let mut found = false;
        super::structured_expression_visit::visit_expression(value, &mut |part| {
            found |= matches!(part, Expression::Variable(read) if read == name);
        });
        found
    }
    if !function.inline_asm_blocks.is_empty() {
        return None;
    }
    let mut rewritten = function.clone();
    let mut changed = false;
    for local in &mut rewritten.locals {
        if local.is_volatile
            || local.is_static
            || local.array_length.is_some()
            || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            || !matches!(local.initializer, Some(Expression::IntegerLiteral(0)))
            || function.locals.iter().any(|other| {
                other
                    .initializer
                    .as_ref()
                    .is_some_and(|value| mentions(value, &local.name))
            })
        {
            continue;
        }
        for statement in &mut rewritten.statements {
            match statement {
                Statement::Assign { name, value } if name == &local.name => {
                    if let Expression::Binary {
                        operator: BinaryOperator::BitOr,
                        left,
                        right,
                    } = value
                    {
                        if matches!(left.as_ref(), Expression::Variable(read) if read == name)
                            && is_negated_call(right)
                            && !mentions(right, name)
                        {
                            *value = right.as_ref().clone();
                            local.initializer = None;
                            changed = true;
                        }
                    }
                    break;
                }
                Statement::Assign { value, .. } | Statement::Expression(value)
                    if !mentions(value, &local.name) => {}
                Statement::Store { target, value }
                    if !mentions(target, &local.name) && !mentions(value, &local.name) => {}
                _ => break,
            }
        }
    }
    changed.then_some(rewritten)
}

pub(super) fn fold_zero_initialized_call_accumulator(function: &Function) -> Option<Function> {
    let statements = &function.statements;
    for index in 0..statements.len().saturating_sub(1) {
        let Statement::Assign {
            name,
            value: Expression::IntegerLiteral(0),
        } = &statements[index]
        else {
            continue;
        };
        let Statement::Assign {
            name: next_name,
            value,
        } = &statements[index + 1]
        else {
            continue;
        };
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left,
            right,
        } = value
        else {
            continue;
        };
        if next_name != name
            || !matches!(left.as_ref(), Expression::Variable(read) if read == name)
            || !is_negated_call(right)
        {
            continue;
        }

        let mut rewritten = function.clone();
        let Statement::Assign { value, .. } = &mut rewritten.statements[index + 1] else {
            unreachable!("matched assignment")
        };
        let Expression::Binary { right, .. } = value else {
            unreachable!("matched accumulator")
        };
        let first_value = right.as_ref().clone();
        rewritten.statements[index + 1] = Statement::Assign {
            name: name.clone(),
            value: first_value,
        };
        rewritten.statements.remove(index);
        return Some(rewritten);
    }
    None
}

fn is_negated_call(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } if is_scalar_call(operand)
    )
}

fn is_scalar_call(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Call { .. } | Expression::CallThrough { .. }
    )
}

fn is_call_accumulator_value(name: &str, value: &Expression) -> bool {
    match value {
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => matches!(operand.as_ref(), Expression::Call { .. }),
        Expression::Binary {
            operator: BinaryOperator::BitOr,
            left,
            right,
        } => {
            matches!(left.as_ref(), Expression::Variable(read) if read == name)
                && is_negated_call(right)
        }
        _ => false,
    }
}

fn reuses_accumulator_value_lane(
    repeated_indirect_member_loop_entry: bool,
    include_previous: bool,
) -> bool {
    repeated_indirect_member_loop_entry && include_previous
}

impl Generator {
    pub(super) fn try_emit_structured_in_place_call_combine(
        &mut self,
        name: &str,
        value: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Expression::Binary {
            operator,
            left,
            right,
        } = value
        else {
            return Ok(false);
        };
        let mut call = right.as_ref();
        while let Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } = call
        {
            call = operand;
        }
        if !matches!(operator, BinaryOperator::BitOr | BinaryOperator::Add)
            || !matches!(left.as_ref(), Expression::Variable(read) if read == name)
            || !matches!(call, Expression::Call { .. })
        {
            return Ok(false);
        }

        self.evaluate(right, Type::Int, Eabi::general_result().number)?;
        self.output
            .instructions
            .push(if *operator == BinaryOperator::Add {
                Instruction::Add {
                    d: destination,
                    a: destination,
                    b: Eabi::general_result().number,
                }
            } else {
                Instruction::Or {
                    a: destination,
                    s: destination,
                    b: Eabi::general_result().number,
                }
            });
        Ok(true)
    }

    pub(super) fn try_emit_structured_call_accumulator(
        &mut self,
        name: &str,
        value: &Expression,
        previous: Option<u8>,
        preference: Option<u8>,
        optimizer_owned_lane: bool,
        shared_home: bool,
    ) -> Compilation<Option<u8>> {
        let (call, include_previous) = match value {
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } if is_scalar_call(operand) => {
                (operand.as_ref(), false)
            }
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(read) if read == name) => {
                let Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand,
                } = right.as_ref()
                else {
                    return Ok(None);
                };
                if !is_scalar_call(operand) {
                    return Ok(None);
                }
                (operand.as_ref(), true)
            }
            _ => return Ok(None),
        };
        let home = previous;
        let previous = if include_previous {
            Some(previous.ok_or_else(|| {
                Diagnostic::error("structured call accumulator is read before its first value")
            })?)
        } else {
            None
        };
        // A repeated inlined callback walk is one mutable reduction in MWCC's
        // value graph. Keep its virtual identity across each `|=` so allocation
        // sees the complete lifetime and can leave the value in r29. Ordinary
        // accumulator chains retain their measured versioned destinations.
        let destination = if shared_home && home.is_some() {
            // A join/backedge must update the incoming identity, even for
            // replacement assignments that do not merge the previous value.
            home.expect("the shared accumulator already has a home")
        } else if reuses_accumulator_value_lane(
            self.structured_repeated_indirect_member_loop_entry,
            include_previous,
        ) {
            let previous = previous.expect("an in-place accumulator has a previous value");
            // This lane is not one of the source-declared saved homes: it is
            // optimizer state introduced by the inlined reduction. Its r29
            // preference leaves the later, non-overlapping source local in
            // the planned r27 home and makes the physical save suffix dense.
            if optimizer_owned_lane {
                self.prefer_virtual_general(previous, 29);
            }
            previous
        } else {
            preference
                .map(|register| self.fresh_virtual_general_preferring(register))
                .unwrap_or_else(|| self.fresh_virtual_general())
        };

        self.evaluate(call, Type::Int, Eabi::general_result().number)?;
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros {
                a: GENERAL_SCRATCH,
                s: Eabi::general_result().number,
            });
        let normalized = if previous.is_some() {
            GENERAL_SCRATCH
        } else {
            destination
        };
        self.output.instructions.push(Instruction::RotateAndMask {
            a: normalized,
            s: GENERAL_SCRATCH,
            shift: 27,
            begin: if self.structured_repeated_indirect_member_loop_entry {
                5
            } else {
                24
            },
            end: 31,
        });
        if let Some(previous) = previous {
            self.output.instructions.push(Instruction::Or {
                a: destination,
                s: previous,
                b: normalized,
            });
        }
        Ok(Some(destination))
    }

    /// Interleave boolean normalization with the following call's independent
    /// argument loads. Calls remain at their original indices, so Rel24 sites do
    /// not move; only the dependency-complete instructions between them do.
    pub(super) fn schedule_structured_call_accumulator_chain(&mut self) {
        let calls: Vec<usize> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(instruction, Instruction::BranchAndLink { .. })
                    .then_some(index)
                    .filter(|index| {
                        matches!(
                            self.output.instructions.get(index + 1),
                            Some(Instruction::CountLeadingZeros { .. })
                        )
                    })
            })
            .collect();
        if calls.len() < 4 {
            return;
        }

        self.schedule_first_accumulator_gap(calls[0] + 1, calls[1]);
        self.schedule_middle_accumulator_gap(calls[1] + 1, calls[2]);
        self.schedule_late_accumulator_gap(calls[2] + 1, calls[3]);
    }

    fn schedule_first_accumulator_gap(&mut self, start: usize, end: usize) {
        if end.saturating_sub(start) != 7 {
            return;
        }
        let window = self.output.instructions[start..end].to_vec();
        if !matches!(window[0], Instruction::CountLeadingZeros { .. })
            || !matches!(window[1], Instruction::RotateAndMask { .. })
            || !matches!(window[3], Instruction::LoadWord { d: 4, .. })
            || !matches!(window[5], Instruction::LoadWord { d: 5, .. })
        {
            return;
        }
        let order = [3, 0, 5, 2, 1, 4, 6];
        self.output.instructions.splice(
            start..end,
            order.into_iter().map(|index| window[index].clone()),
        );
    }

    fn schedule_middle_accumulator_gap(&mut self, start: usize, end: usize) {
        if end.saturating_sub(start) != 7 {
            return;
        }
        let window = self.output.instructions[start..end].to_vec();
        if !matches!(window[0], Instruction::CountLeadingZeros { .. })
            || !matches!(window[1], Instruction::RotateAndMask { .. })
            || !matches!(window[2], Instruction::Or { .. })
        {
            return;
        }
        let order: &[usize] = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst
                if self.behavior.power_pc_7400_scheduling_enabled() =>
            {
                &[0, 3, 1, 4, 5, 2, 6]
            }
            FrameConvention::LinkageFirst => &[0, 1, 3, 4, 5, 2, 6],
            FrameConvention::Predecrement => &[0, 3, 1, 4, 5, 6, 2],
        };
        self.output.instructions.splice(
            start..end,
            order.iter().map(|index| window[*index].clone()),
        );
    }

    fn schedule_late_accumulator_gap(&mut self, start: usize, end: usize) {
        if end.saturating_sub(start) != 4 {
            return;
        }
        let window = self.output.instructions[start..end].to_vec();
        if !matches!(window[0], Instruction::CountLeadingZeros { .. })
            || !matches!(window[1], Instruction::RotateAndMask { .. })
            || !matches!(window[2], Instruction::Or { .. })
        {
            return;
        }
        let order: &[usize] = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst
                if self.behavior.power_pc_7400_scheduling_enabled() =>
            {
                &[0, 3, 1, 2]
            }
            FrameConvention::LinkageFirst => &[0, 1, 3, 2],
            FrameConvention::Predecrement => &[0, 3, 1, 2],
        };
        self.output.instructions.splice(
            start..end,
            order.iter().map(|index| window[*index].clone()),
        );
    }

    /// Replace the generic ternary materialization after the final accumulated
    /// call with MWCC's frame-generation-specific terminal form.
    pub(super) fn lower_structured_call_accumulator_return(&mut self) -> bool {
        let instructions = &self.output.instructions;
        if instructions.len() < 8 {
            return false;
        }
        let start = instructions.len() - 8;
        let window = &instructions[start..];
        let (
            Instruction::CountLeadingZeros { s: 3, .. },
            Instruction::RotateAndMask { .. },
            Instruction::Or {
                s: previous,
                b: 0,
                ..
            },
        ) = (&window[0], &window[1], &window[2])
        else {
            return false;
        };
        if !matches!(window[3], Instruction::Negate { d: 3, .. })
            || !matches!(window[4], Instruction::AddImmediate { d: 0, a: 0, immediate: -3 })
            || !matches!(window[5], Instruction::Or { a: 3, .. })
            || !matches!(window[6], Instruction::ShiftRightAlgebraicImmediate { a: 3, .. })
            || !matches!(window[7], Instruction::And { a: 3, .. })
        {
            return false;
        }
        let previous = *previous;
        let replacement = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                vec![
                    Instruction::CountLeadingZeros { a: 0, s: 3 },
                    Instruction::RotateAndMask {
                        a: 0,
                        s: 0,
                        shift: 27,
                        begin: 24,
                        end: 31,
                    },
                    Instruction::OrRecord {
                        a: previous,
                        s: previous,
                        b: 0,
                    },
                    Instruction::BranchConditionalForward {
                        options: 12,
                        condition_bit: 2,
                        target: start + 6,
                    },
                    Instruction::load_immediate(3, -3),
                    Instruction::Branch { target: start + 7 },
                    Instruction::load_immediate(3, 0),
                ]
            }
            FrameConvention::Predecrement => vec![
                Instruction::CountLeadingZeros { a: 3, s: 3 },
                Instruction::load_immediate(0, -3),
                Instruction::RotateAndMask {
                    a: 3,
                    s: 3,
                    shift: 27,
                    begin: 24,
                    end: 31,
                },
                Instruction::Or {
                    a: previous,
                    s: previous,
                    b: 3,
                },
                Instruction::SubtractFromImmediate {
                    d: 3,
                    a: previous,
                    immediate: 0,
                },
                Instruction::SubtractFromExtended { d: 3, a: 3, b: 3 },
                Instruction::And { a: 3, s: 0, b: 3 },
            ],
        };
        self.output.instructions.splice(start.., replacement);
        true
    }
}

#[cfg(test)]
mod value_lane_tests {
    use super::*;

    #[test]
    fn discovers_an_accumulator_nested_in_a_loop_body() {
        let statements = vec![Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![Statement::Assign {
                name: "error".to_owned(),
                value: Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: Box::new(Expression::Variable("error".to_owned())),
                    right: Box::new(Expression::Unary {
                        operator: UnaryOperator::LogicalNot,
                        operand: Box::new(Expression::Call {
                            name: "operation".to_owned(),
                            arguments: Vec::new(),
                        }),
                    }),
                },
            }],
        }];
        let mut names = std::collections::HashSet::new();

        collect_call_accumulator_names(&statements, &mut names);

        assert_eq!(names, std::collections::HashSet::from(["error"]));
        assert_eq!(count_call_accumulator_assignments(&statements), 1);
    }

    #[test]
    fn repeated_indirect_walks_keep_their_reduction_in_one_value_lane() {
        assert!(reuses_accumulator_value_lane(true, true));
        assert!(!reuses_accumulator_value_lane(true, false));
        assert!(!reuses_accumulator_value_lane(false, true));
    }
}

#[cfg(test)]
mod entry_zero_tests {
    use super::*;

    fn variable() -> Expression {
        Expression::Variable("error".into())
    }
    fn call(arguments: Vec<Expression>) -> Expression {
        Expression::Call {
            name: "operation".into(),
            arguments,
        }
    }
    fn sample(prefix: Vec<Statement>) -> Function {
        let mut statements = prefix;
        statements.push(Statement::Assign {
            name: "error".into(),
            value: Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: Box::new(variable()),
                right: Box::new(Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: Box::new(call(Vec::new())),
                }),
            },
        });
        Function {
            return_type: Type::Int,
            name: "transaction".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "error".into(),
                initializer: Some(Expression::IntegerLiteral(0)),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements,
            guards: Vec::new(),
            return_expression: Some(variable()),
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
    fn folds_across_an_unrelated_call_without_removing_its_effects() {
        let prefix = Statement::Expression(call(vec![Expression::IntegerLiteral(7)]));
        let function = sample(vec![prefix.clone()]);
        let folded = fold_entry_zero_call_accumulators(&function).unwrap();
        assert!(folded.locals[0].initializer.is_none());
        assert_eq!(folded.statements.len(), 2);
        assert_eq!(format!("{:?}", folded.statements[0]), format!("{prefix:?}"));
        assert!(
            matches!(&folded.statements[1], Statement::Assign { value, .. } if is_negated_call(value))
        );
    }

    #[test]
    fn retains_initialization_when_the_prefix_reads_modifies_or_exposes_it() {
        for prefix in [
            Statement::Expression(call(vec![variable()])),
            Statement::Expression(call(vec![Expression::AddressOf {
                operand: Box::new(variable()),
            }])),
            Statement::Expression(Expression::Assign {
                target: Box::new(variable()),
                value: Box::new(Expression::IntegerLiteral(2)),
            }),
            Statement::Expression(Expression::AggregateLiteral(vec![variable()])),
            Statement::Label("again".into()),
            Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ] {
            assert!(fold_entry_zero_call_accumulators(&sample(vec![prefix])).is_none());
        }
    }

    #[test]
    fn rejects_volatile_static_nonzero_and_initializer_aliases() {
        let mut volatile = sample(Vec::new());
        volatile.locals[0].is_volatile = true;
        let mut persistent = sample(Vec::new());
        persistent.locals[0].is_static = true;
        let mut nonzero = sample(Vec::new());
        nonzero.locals[0].initializer = Some(Expression::IntegerLiteral(2));
        let mut alias = sample(Vec::new());
        let mut other = alias.locals[0].clone();
        other.name = "alias".into();
        other.initializer = Some(Expression::AddressOf {
            operand: Box::new(variable()),
        });
        alias.locals.push(other);
        for function in [volatile, persistent, nonzero, alias] {
            assert!(fold_entry_zero_call_accumulators(&function).is_none());
        }
    }

    #[test]
    fn retains_zero_when_the_first_operation_takes_its_address() {
        let mut function = sample(Vec::new());
        let Statement::Assign {
            value: Expression::Binary { right, .. },
            ..
        } = &mut function.statements[0]
        else {
            unreachable!()
        };
        *right = Box::new(Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(call(vec![Expression::AddressOf {
                operand: Box::new(variable()),
            }])),
        });
        assert!(fold_entry_zero_call_accumulators(&function).is_none());
    }
}
