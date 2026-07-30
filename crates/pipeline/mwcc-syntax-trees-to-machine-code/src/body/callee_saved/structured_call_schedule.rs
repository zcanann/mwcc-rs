//! Final-use call-argument encodings in allocator-owned structured bodies.

use super::structured_locals::body_uses_local;
use mwcc_syntax_trees::Parameter;
#[allow(unused_imports)]
use super::*;

fn direct_callback_wait_entry(function: &Function) -> Option<(&str, &str, &str)> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(parameter.parameter_type, Type::StructPointer { .. })
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let Statement::Assign {
        name: result,
        value:
            Expression::Call {
                name: starter,
                arguments,
            },
    } = function.statements.first()?
    else {
        return None;
    };
    let [receiver, Expression::Variable(callback)] = arguments.as_slice() else {
        return None;
    };
    fn receiver_name(expression: &Expression) -> Option<&str> {
        match expression {
            Expression::Variable(name) => Some(name),
            Expression::Cast { operand, .. } => receiver_name(operand),
            _ => None,
        }
    }
    if receiver_name(receiver) != Some(parameter.name.as_str()) {
        return None;
    }
    let rejects_zero = function.statements.get(1).is_some_and(|statement| {
        matches!(statement, Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
            then_body,
            else_body,
        } if else_body.is_empty()
            && matches!(left.as_ref(), Expression::Variable(name) if name == result)
            && constant_value(right) == Some(0)
            && matches!(then_body.as_slice(), [Statement::Return(Some(value))]
                if constant_value(value) == Some(-1)))
    });
    rejects_zero.then_some((parameter.name.as_str(), callback.as_str(), starter.as_str()))
}

pub(super) fn direct_callback_wait_home_preference(
    function: &Function,
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    first_saved: usize,
    home_index: usize,
) -> Option<u8> {
    (saved_parameters.len() == 1
        && deferred_saved_locals.len() == 1
        && direct_callback_wait_entry(function)
            .is_some_and(|(receiver, _, _)| receiver == saved_parameters[0].name))
    .then(|| u8::try_from(first_saved + home_index).ok())
    .flatten()
}

/// Lay out a larger inlined async transaction's retained receiver, identifier,
/// and wait state in ascending saved-register order.
///
/// The first comma sequence owns a diagnostic branch and ends in the queueing
/// call; the following guard rejects a zero result before entering a blocking
/// wait loop. The identifier dies after publication, so the later return value
/// may reuse its middle home while the interrupt token occupies the high home.
pub(super) fn sequenced_callback_wait_home_preference(
    function: &Function,
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    first_saved: usize,
    home_index: usize,
) -> Option<u8> {
    if !is_sequenced_callback_wait_layout(
        function,
        saved_parameters,
        deferred_saved_locals,
        first_saved,
    ) {
        return None;
    }
    let [receiver, _identifier] = function.parameters.as_slice() else {
        unreachable!("the layout recognizer requires two parameters")
    };
    let preferred = saved_parameters
        .get(home_index)
        .map(|parameter| {
            if parameter.name == receiver.name {
                first_saved
            } else {
                first_saved + 1
            }
        })
        .unwrap_or(first_saved + 2);
    u8::try_from(preferred).ok()
}

pub(super) fn is_sequenced_callback_wait_layout(
    function: &Function,
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    first_saved: usize,
) -> bool {
    let [receiver, identifier] = function.parameters.as_slice() else {
        return false;
    };
    let Some(Statement::Assign {
        name: result,
        value: sequence @ Expression::Comma { .. },
    }) = function.statements.first()
    else {
        return false;
    };
    let Some((_, arguments)) = terminal_call(sequence) else {
        return false;
    };
    let terminal_receiver = arguments.last().and_then(variable_name);
    saved_parameters.len() == 2
        && deferred_saved_locals.len() == 2
        && first_saved == 29
        && saved_parameters
            .iter()
            .all(|parameter| parameter.name == receiver.name || parameter.name == identifier.name)
        && terminal_receiver == Some(receiver.name.as_str())
        && sequence_contains_conditional(sequence)
        && function
            .statements
            .get(1)
            .is_some_and(|statement| rejects_zero_result(statement, result))
}

/// Source-home indices in physical frame-slot order for a sequenced callback
/// wait. The deferred interrupt state owns the top slot, followed by the
/// identifier and receiver parameter homes.
pub(super) const fn sequenced_callback_wait_save_order() -> [usize; 3] {
    [2, 0, 1]
}

pub(super) fn sequenced_callback_wait_frame_slot(home_index: usize) -> Option<usize> {
    sequenced_callback_wait_save_order()
        .iter()
        .position(|candidate| *candidate == home_index)
}

fn terminal_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    match expression {
        Expression::Call { name, arguments } => Some((name, arguments)),
        Expression::Comma { right, .. } => terminal_call(right),
        _ => None,
    }
}

fn variable_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable_name(operand),
        _ => None,
    }
}

fn sequence_contains_conditional(expression: &Expression) -> bool {
    match expression {
        Expression::Conditional { .. } => true,
        Expression::Comma { left, right } => {
            sequence_contains_conditional(left) || sequence_contains_conditional(right)
        }
        _ => false,
    }
}

fn rejects_zero_result(statement: &Statement, result: &str) -> bool {
    matches!(statement, Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        },
        then_body,
        else_body,
    } if else_body.is_empty()
        && matches!(left.as_ref(), Expression::Variable(name) if name == result)
        && constant_value(right) == Some(0)
        && matches!(then_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(-1)))
}

pub(super) fn transient_call_argument_register(
    statements: &[Statement],
    candidate: &str,
) -> Option<u8> {
    statements
        .iter()
        .find_map(|statement| statement_call_argument_index(statement, candidate))
        .and_then(|index| u8::try_from(index).ok())
        .and_then(|index| Eabi::FIRST_GENERAL_ARGUMENT.checked_add(index))
        .filter(|register| *register <= 10)
}

/// Select the ABI home when a terminal offset computation is consumed directly
/// by the immediately following call. MWCC forms these in the argument register
/// instead of retaining the reassigned local's older saved-register home.
pub(super) fn terminal_offset_call_argument_register(
    value: &Expression,
    next: Option<&Statement>,
    candidate: &str,
) -> Option<u8> {
    let is_offset = matches!(
        value,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if (matches!(left.as_ref(), Expression::Variable(_))
            && constant_value(right).is_some())
            || (constant_value(left).is_some()
                && matches!(right.as_ref(), Expression::Variable(_)))
    ) || matches!(
        value,
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(_))
            && constant_value(right).is_some()
    );
    is_offset
        .then(|| next.and_then(|statement| statement_call_argument_index(statement, candidate)))
        .flatten()
        .and_then(|index| u8::try_from(index).ok())
        .and_then(|index| Eabi::FIRST_GENERAL_ARGUMENT.checked_add(index))
        .filter(|register| *register <= 10)
}

fn statement_call_argument_index(statement: &Statement, candidate: &str) -> Option<usize> {
    match statement {
        Statement::Store { target, value } => expression_call_argument_index(target, candidate)
            .or_else(|| expression_call_argument_index(value, candidate)),
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => expression_call_argument_index(value, candidate),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => expression_call_argument_index(condition, candidate).or_else(|| {
            then_body
                .iter()
                .chain(else_body)
                .find_map(|statement| statement_call_argument_index(statement, candidate))
        }),
        _ => None,
    }
}

fn expression_call_argument_index(expression: &Expression, candidate: &str) -> Option<usize> {
    match expression {
        Expression::Call { arguments, .. } => arguments.iter().position(
            |argument| matches!(argument, Expression::Variable(name) if name == candidate),
        ),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => expression_call_argument_index(left, candidate)
            .or_else(|| expression_call_argument_index(right, candidate)),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::PostStep {
            target: operand, ..
        } => expression_call_argument_index(operand, candidate),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => expression_call_argument_index(condition, candidate)
            .or_else(|| expression_call_argument_index(when_true, candidate))
            .or_else(|| expression_call_argument_index(when_false, candidate)),
        _ => None,
    }
}

impl Generator {
    /// Schedule a direct async starter's receiver and callback address through
    /// the linkage latency slots. The receiver remains live in its lower saved
    /// home while the interrupt token introduced after the call takes the next
    /// home; r3 still contains the entry receiver, so its reload is dead.
    pub(crate) fn schedule_direct_callback_wait_entry(&mut self, function: &Function) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || direct_callback_wait_entry(function).is_none()
        {
            return;
        }
        if !matches!(
            self.output.instructions.get(0..10),
            Some([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -24,
                },
                Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 20,
                },
                Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 16,
                },
                Instruction::AddImmediate {
                    d: 30,
                    a: 3,
                    immediate: 0,
                },
                Instruction::Or { a: 3, s: 30, b: 30 },
                Instruction::AddImmediateShifted {
                    d: 4,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ])
        ) {
            return;
        }

        crate::remove_instruction_retargeting_to_next(self, 6);
        crate::move_instruction_before_retargeting(self, 6, 1);
        crate::move_instruction_before_retargeting(self, 7, 3);
        if matches!(
            self.output.instructions.get(14),
            Some(Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            })
        ) {
            self.output.instructions[14] = Instruction::move_register(31, 3);
        }
    }

    /// Hoist the saved-LR load into the issue slot before a final move from a
    /// callee-saved return home. The load is independent of the result move;
    /// MWCC uses that latency schedule before restoring the saved GPR range.
    pub(crate) fn schedule_saved_return_epilogue(&mut self, function: &Function) {
        // A switch-selected return value is already the output phi of the
        // dispatch transaction. Build 163 publishes that value to r3 before
        // beginning linkage teardown; moving the LR load ahead of it applies
        // the schedule for ordinary saved locals to the wrong provenance.
        if function.return_expression.as_ref().is_some_and(|returned| {
            matches!(returned, Expression::Variable(name)
                if assigned_by_source_switch(&function.statements, name))
        }) {
            return;
        }
        // An owner that explicitly selected LR-before-GPR teardown has already
        // fixed the issue order: return handoff, LR reload, then GPR restores.
        if self.epilogue_lr_before_gprs {
            return;
        }
        let Some(lr_index) = self.output.instructions.iter().rposition(|instruction| {
            matches!(instruction, Instruction::LoadWord { d: 0, a: 1, offset } if *offset == self.frame_size + 4)
        }) else {
            return;
        };
        let Some(return_index) = lr_index.checked_sub(1) else {
            return;
        };
        let return_source = match self.output.instructions[return_index] {
            Instruction::Or { a: 3, s, b } if s == b => s,
            _ => return,
        };
        // Before allocation, the planned home is identified by
        // `callee_saved`; afterward, the matching stack restore is the
        // authoritative proof. Supporting both lets this narrow schedule run
        // on either side of terminal-branch cleanup.
        let restores_return_source = self.output.instructions[lr_index + 1..]
            .iter()
            .any(|instruction| {
                matches!(instruction, Instruction::LoadWord { d, a: 1, .. } if *d == return_source)
            });
        if !self.callee_saved.contains(&return_source) && !restores_return_source {
            return;
        }
        self.output.instructions.swap(return_index, lr_index);
        self.labels.moved_before(lr_index, return_index);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == return_index => lr_index,
                index if index == lr_index => return_index,
                index => index,
            };
        }
    }

    /// Complete the paired entry-call schedule by restoring LR before the one
    /// saved receiver. The entry shape is the proof that this ordering applies;
    /// unrelated one-register frames retain the generic epilogue.
    pub(super) fn schedule_saved_receiver_entry_epilogue(&mut self) {
        if self.callee_saved.len() != 1
            || self.output.instructions.len() < 11
            || !matches!(&self.output.instructions[..6], [
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::StoreWord { s, a: 1, .. },
                Instruction::Or { a, s: 3, b: 3 },
            ] if *s == self.callee_saved[0] && *a == self.callee_saved[0])
        {
            return;
        }
        let end = self.output.instructions.len();
        if matches!(&self.output.instructions[end - 5..], [
            Instruction::LoadWord { d: saved, a: 1, .. },
            Instruction::LoadWord { d: 0, a: 1, .. },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, .. },
            Instruction::BranchToLinkRegister,
        ] if *saved == self.callee_saved[0])
        {
            self.output.instructions.swap(end - 5, end - 4);
        }
    }

    /// A first call through a receiver just promoted from r3 into a saved home
    /// does not need to copy that receiver back to r3: r3 still contains the
    /// entry value. MWCC uses the freed issue slot to materialize a literal
    /// second argument between `mflr` and the two prologue stores.
    pub(super) fn schedule_saved_receiver_entry_call(
        &mut self,
        statement: &Statement,
        function: &Function,
        statement_index: usize,
        emitted_start: usize,
    ) {
        if statement_index != 0
            || self.behavior.frame_convention != FrameConvention::Predecrement
            || self.callee_saved.len() != 1
        {
            return;
        }
        let Statement::Expression(expression) = statement else {
            return;
        };
        let Some(arguments) = leading_call_arguments(expression) else {
            return;
        };
        let [Expression::Variable(receiver), Expression::IntegerLiteral(literal)] = arguments
        else {
            return;
        };
        if function
            .parameters
            .first()
            .map(|parameter| parameter.name.as_str())
            != Some(receiver.as_str())
            || self.lookup_general(receiver) != self.callee_saved.first().copied()
        {
            return;
        }
        let prefix = &self.output.instructions;
        if emitted_start != 5
            || prefix.len() < 8
            || !matches!(&prefix[..8], [
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::StoreWord { s, a: 1, .. },
                Instruction::Or { a, s: source, b },
                Instruction::Or { a: 3, s: call_source, b: call_source_b },
                Instruction::AddImmediate { d: 4, a: 0, immediate },
                Instruction::BranchAndLink { .. },
            ] if *s == self.callee_saved[0]
                && *a == self.callee_saved[0]
                && *source == 3
                && *b == 3
                && *call_source == self.callee_saved[0]
                && *call_source_b == self.callee_saved[0]
                && i64::from(*immediate) == *literal)
        {
            return;
        }
        self.output.instructions.remove(5);
        self.labels.removed(5, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > 5 {
                relocation.instruction_index -= 1;
            }
        }
        let literal_load = self.output.instructions.remove(5);
        self.labels.moved_before(5, 2);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == 5 {
                2
            } else if (2..5).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
        self.output.instructions.insert(2, literal_load);
    }

    /// Build 163 keeps the power-of-two product in r3 while forming a second
    /// call argument such as `consume(data, length * 8 + 1)`. The generic
    /// immediate selector coalesces both operations into the argument home;
    /// split the producer lifetime so allocation can retain MWCC's intermediate.
    pub(super) fn stage_legacy_shift_add_call_argument(
        &mut self,
        statement: &Statement,
        remaining: &[Statement],
        emitted_start: usize,
    ) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Statement::Assign { name, value } = statement else {
            return;
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = value
        else {
            return;
        };
        if !matches!(
            (left.as_ref(), right.as_ref()),
            (
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    right,
                    ..
                },
                Expression::IntegerLiteral(_)
            ) if crate::analysis::constant_value(right).is_some_and(|factor| {
                factor > 1 && (factor as u64).is_power_of_two()
            })
        ) {
            return;
        }
        if remaining
            .first()
            .and_then(|next| statement_call_argument_index(next, name))
            != Some(1)
        {
            return;
        }
        let Some(home) = self.lookup_general(name) else {
            return;
        };
        if !is_coalesced_shift_add_window(&self.output.instructions[emitted_start..], home) {
            return;
        }
        let preferred = if self.behavior.power_pc_7400_scheduling_enabled() {
            Eabi::FIRST_GENERAL_ARGUMENT + 1
        } else {
            Eabi::FIRST_GENERAL_ARGUMENT
        };
        let staged = self.fresh_virtual_general_preferring(preferred);
        let [Instruction::ShiftLeftImmediate { a, .. }, Instruction::AddImmediate { a: source, .. }] =
            &mut self.output.instructions[emitted_start..]
        else {
            unreachable!("window checked above");
        };
        *a = staged;
        *source = staged;
    }

    /// Build 163 spells the final multi-argument forwarding of a deferred local
    /// home as `addi d,s,0`, while earlier uses and entry-initialized locals
    /// remain `mr`. Selection cannot infer this provenance from the virtual
    /// register number, so the structured statement owner applies the encoding
    /// only after proving this call is the deferred local's final use.
    pub(super) fn schedule_dying_structured_local_argument(
        &mut self,
        statement: &Statement,
        remaining: &[Statement],
        function: &Function,
        emitted_start: usize,
    ) {
        if self.behavior.materialization_copy_style
            != mwcc_versions::MaterializationCopyStyle::AddImmediateZero
        {
            return;
        }
        let Some(name) = dying_first_local_argument(statement, remaining, &self.known_locals)
        else {
            return;
        };
        if !function
            .locals
            .iter()
            .any(|local| local.name == name && local.initializer.is_none())
        {
            return;
        }
        let Some(source) = self.lookup_general(name) else {
            return;
        };
        let candidates: Vec<usize> = self.output.instructions[emitted_start..]
            .iter()
            .enumerate()
            .filter_map(|(offset, instruction)| {
                matches!(instruction, Instruction::Or { a: 3, s, b } if *s == source && *b == source)
                    .then_some(emitted_start + offset)
            })
            .collect();
        let [copy] = candidates.as_slice() else {
            return;
        };
        self.output.instructions[*copy] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT,
            a: source,
            immediate: 0,
        };
    }
}

fn assigned_by_source_switch(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    block_assigns_name(statements, name)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            }) || default.as_ref().is_some_and(|default| match default {
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    block_assigns_name(statements, name)
                }
                mwcc_syntax_trees::ArmBody::Return(_) => false,
            })
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            assigned_by_source_switch(then_body, name)
                || assigned_by_source_switch(else_body, name)
        }
        Statement::Loop { body, .. } => assigned_by_source_switch(body, name),
        _ => false,
    })
}

fn block_assigns_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => block_assigns_name(then_body, name) || block_assigns_name(else_body, name),
        Statement::Loop { body, .. } => block_assigns_name(body, name),
        Statement::Switch { .. } => assigned_by_source_switch(
            std::slice::from_ref(statement),
            name,
        ),
        _ => false,
    })
}

fn leading_call_arguments(expression: &Expression) -> Option<&[Expression]> {
    match expression {
        Expression::Call { arguments, .. } => Some(arguments),
        Expression::Comma { left, .. } => leading_call_arguments(left),
        _ => None,
    }
}

fn dying_first_local_argument<'a>(
    statement: &'a Statement,
    remaining: &[Statement],
    known_locals: &std::collections::HashSet<String>,
) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
        return None;
    };
    let [Expression::Variable(name), _, ..] = arguments.as_slice() else {
        return None;
    };
    (known_locals.contains(name) && !body_uses_local(remaining, name)).then_some(name)
}

fn is_coalesced_shift_add_window(instructions: &[Instruction], home: u8) -> bool {
    matches!(
        instructions,
        [
            Instruction::ShiftLeftImmediate { a, .. },
            Instruction::AddImmediate {
                d,
                a: source,
                ..
            }
        ] if *a == home && *d == home && *source == home
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequenced_callback_wait_save_order_and_slots_are_inverses() {
        for (slot, home_index) in sequenced_callback_wait_save_order().into_iter().enumerate() {
            assert_eq!(
                sequenced_callback_wait_frame_slot(home_index),
                Some(slot),
            );
        }
        assert_eq!(sequenced_callback_wait_frame_slot(3), None);
    }

    #[test]
    fn recognizes_a_coalesced_shift_add_argument() {
        let instructions = [
            Instruction::ShiftLeftImmediate {
                a: 40,
                s: 41,
                shift: 3,
            },
            Instruction::AddImmediate {
                d: 40,
                a: 40,
                immediate: 1,
            },
        ];
        assert!(is_coalesced_shift_add_window(&instructions, 40));
        assert!(!is_coalesced_shift_add_window(&instructions, 41));
    }

    fn call(arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments,
        })
    }

    #[test]
    fn distinguishes_final_multi_argument_local_forwarding() {
        let current = call(vec![
            Expression::Variable("local".into()),
            Expression::IntegerLiteral(0),
        ]);
        let known = std::collections::HashSet::from(["local".to_string()]);
        assert_eq!(
            dying_first_local_argument(&current, &[], &known),
            Some("local")
        );

        let later_use = vec![call(vec![Expression::Variable("local".into())])];
        assert_eq!(
            dying_first_local_argument(&current, &later_use, &known),
            None
        );
        assert_eq!(dying_first_local_argument(&later_use[0], &[], &known), None);
    }

    #[test]
    fn selects_the_eabi_register_for_a_forwarded_argument() {
        let statement = call(vec![
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::Variable("length".into()),
        ]);
        assert_eq!(
            transient_call_argument_register(&[statement], "length"),
            Some(6),
        );
    }

    #[test]
    fn selects_an_immediate_terminal_offset_argument() {
        let offset = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable("dummy".into())),
            right: Box::new(Expression::IntegerLiteral(20)),
        };
        let statement = call(vec![
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::IntegerLiteral(0),
            Expression::Variable("length".into()),
        ]);
        assert_eq!(
            terminal_offset_call_argument_register(&offset, Some(&statement), "length"),
            Some(6),
        );
        assert_eq!(
            terminal_offset_call_argument_register(
                &Expression::Variable("dummy".into()),
                Some(&statement),
                "length",
            ),
            None,
        );
    }

    #[test]
    fn recognizes_a_return_local_assigned_inside_a_source_switch() {
        let statements = vec![Statement::Switch {
            scrutinee: Expression::Variable("state".into()),
            arms: vec![mwcc_syntax_trees::SwitchArm {
                value: 0,
                body: mwcc_syntax_trees::ArmBody::Statements(vec![
                    Statement::Assign {
                        name: "result".into(),
                        value: Expression::IntegerLiteral(1),
                    },
                ]),
                falls_through: false,
            }],
            default: None,
        }];
        assert!(assigned_by_source_switch(&statements, "result"));
        assert!(!assigned_by_source_switch(&statements, "other"));
    }
}
