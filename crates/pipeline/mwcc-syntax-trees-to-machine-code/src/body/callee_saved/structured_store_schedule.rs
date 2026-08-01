//! Cross-statement scheduling for structured legacy bodies.
//!
//! A floating call result cannot be stored until the call completes. Build 163
//! fills that latency slot with an independent register truth test from the next
//! statement, then issues the store before the dependent branch. This module
//! recognizes only the dependency-safe source pair and verifies the emitted
//! adjacent instructions before exchanging them.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationKind, RelocationTarget};
#[cfg(test)]
use mwcc_machine_code::Relocation;

impl Generator {
    /// Fill the interval between repeated scaled allocation calls. The next
    /// count load and zero argument are independent of the previous result,
    /// so MWCC issues them before publishing that result to its global:
    /// `bl; lwz count; li zero; stw result; lwz heap; slwi; bl`.
    pub(crate) fn schedule_scaled_global_allocation_publications(&mut self) {
        while let Some(start) = scaled_global_allocation_publication_start(
            &self.output.instructions,
        ) {
            self.move_instruction_before(start + 3, start + 1);
            self.move_instruction_before(start + 3, start + 2);
        }
    }

    /// Schedule two indexed allocation-result publications sharing a retained
    /// zero argument. The second zero is issued in the first result store's
    /// latency interval, while the second size literal precedes its heap load:
    /// `lwz table; li zero; stwx; li size; lwz heap; bl`.
    pub(crate) fn schedule_indexed_allocation_pair(&mut self) {
        let Some(start) = indexed_allocation_pair_start(&self.output.instructions) else {
            return;
        };
        self.move_instruction_before(start + 4, start + 1);
        self.move_instruction_before(start + 4, start + 3);
    }

    /// Fill the indexed store's table-load latency slot with the independent
    /// source induction increment. The byte cursor remains after the store
    /// because it supplies that store's indexed address.
    pub(crate) fn schedule_pointer_table_index_cursor_publication(&mut self) {
        if !self.structured_pointer_table_index_cursor {
            return;
        }
        let Some(start) = pointer_table_index_cursor_publication_start(
            &self.output.instructions,
        ) else {
            return;
        };
        self.move_instruction_before(start + 2, start + 1);
    }

    /// Issue the pointer-table base load before copying the retained search
    /// argument into r3, filling the base load's latency before the indexed load.
    pub(crate) fn schedule_pointer_table_index_cursor_lookup(&mut self) {
        if !self.structured_pointer_table_index_cursor {
            return;
        }
        let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::Or { a: 3, s: argument, b },
                    Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                    Instruction::LoadWordIndexed { d: 4, a: 4, .. },
                    Instruction::BranchAndLink { .. },
                ] if argument == b
            )
        }) else {
            return;
        };
        self.move_instruction_before(start + 1, start);
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == start + 1 =>
                {
                    *target = start;
                }
                _ => {}
            }
        }
    }

    /// After a call, issue a later independent global load before publishing a
    /// zero and retain the zero in r3. This fills the load latency while keeping
    /// r0 available for the loaded value's following global publication.
    pub(crate) fn schedule_post_call_zero_global_publication(&mut self) {
        while let Some(start) =
            post_call_zero_global_publication_start(&self.output)
        {
            self.move_instruction_before(start + 2, start);
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[start + 1]
            else {
                unreachable!("zero publication materialization was recognized")
            };
            *d = Eabi::FIRST_GENERAL_ARGUMENT;
            let Instruction::StoreWord { s, .. } =
                &mut self.output.instructions[start + 2]
            else {
                unreachable!("zero publication store was recognized")
            };
            *s = Eabi::FIRST_GENERAL_ARGUMENT;
        }
    }

    /// Keep a later narrow load out of the integer scratch while a lock value
    /// spans a repeated floating-zero reset. MWCC materializes the lock in the
    /// `lfs` latency slot, loads the narrow value into a fresh volatile home,
    /// and stores the still-live lock after that load.
    pub(super) fn schedule_post_call_jump_state_reset(&mut self) {
        let Some(start) = post_call_jump_state_reset_start(&self.output.instructions) else {
            return;
        };
        if !schedule_relocations::same_relocated_value(
            &self.output.relocations,
            &self.output.constants,
            start + 2,
            start + 4,
        ) {
            return;
        }
        self.move_instruction_before(start + 8, start + 2);
        let narrow = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start + 7] else {
            unreachable!("jump-state reset load was recognized")
        };
        *d = narrow;
        let Instruction::StoreByte { s, .. } = &mut self.output.instructions[start + 8] else {
            unreachable!("jump-state reset store was recognized")
        };
        *s = narrow;
    }

    /// Lead a guarded mixed integer/float call with its independent float
    /// literal load. The receiver's saved-home copy uses `addi` in this
    /// schedule, and the enclosing nested early-return graph contributes six
    /// optimizer labels before the literal pool.
    pub(super) fn schedule_guarded_saved_receiver_float_call(&mut self) {
        let Some(start) =
            guarded_saved_receiver_float_call_start(&self.output.instructions)
        else {
            return;
        };
        self.move_instruction_before(start + 3, start + 1);
        let saved = match self.output.instructions[start + 2] {
            Instruction::Or { s, b, .. } if s == b => s,
            _ => unreachable!("guarded saved receiver call shape was checked"),
        };
        self.output.instructions[start + 2] = Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT,
            a: saved,
            immediate: 0,
        };
        self.output.anonymous_label_bump += 6;
    }

    /// Fill the load-to-use gap of an inlined two-component float scale with
    /// the entry receiver needed by the immediately following call.
    pub(super) fn schedule_inline_float_pair_final_call(&mut self) {
        let Some(start) = self.output.instructions.windows(9).position(|window| {
            matches!(window, [
                Instruction::LoadFloatSingle { d: scale, a: owner, .. },
                Instruction::LoadFloatSingle { d: first, a: first_base, .. },
                Instruction::FloatMultiplySingle { d: first_product, a: first_value, c: first_scale },
                Instruction::StoreFloatSingle { s: first_stored, a: first_store_base, .. },
                Instruction::LoadFloatSingle { d: second, a: second_base, .. },
                Instruction::FloatMultiplySingle { d: second_product, a: second_value, c: second_scale },
                Instruction::StoreFloatSingle { s: second_stored, a: second_store_base, .. },
                Instruction::Or { a: 3, s: receiver, b },
                Instruction::BranchAndLink { .. },
            ] if owner == first_base
                && owner == first_store_base
                && owner == second_base
                && owner == second_store_base
                && scale == first_scale
                && scale == second_scale
                && first == first_product
                && first == first_value
                && first == first_stored
                && second == second_product
                && second == second_value
                && second == second_stored
                && receiver == b)
        }) else {
            return;
        };
        self.move_instruction_before(start + 7, start + 1);
    }

    /// Hoist an independent three-register call setup across a run of three
    /// stores from one already-loaded float. MWCC fills the store issue window
    /// this way in constructor-like state initialization bodies.
    pub(super) fn schedule_structured_float_store_call_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        if let Some(start) = self.output.instructions.windows(8).position(|window| {
            matches!(window, [
                Instruction::LoadFloatSingle { d: loaded, .. },
                Instruction::StoreFloatSingle { s: first, a: first_base, .. },
                Instruction::StoreFloatSingle { s: second, a: second_base, .. },
                Instruction::StoreFloatSingle { s: third, a: third_base, .. },
                Instruction::Or { a: 3, s: receiver, b },
                Instruction::AddImmediate { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 5, a: 0, .. },
                Instruction::BranchAndLink { .. },
            ] if loaded == first
                && first == second
                && second == third
                && first_base == second_base
                && second_base == third_base
                && receiver == first_base
                && b == receiver)
        }) {
            for offset in 0..3 {
                self.move_instruction_before(start + 4 + offset, start + 1 + offset);
            }
        }

        // The one-store sibling fills the load-to-store latency slot with the
        // receiver copy for the immediately following call.
        if let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(window, [
                Instruction::LoadFloatSingle { d: loaded, .. },
                Instruction::StoreFloatSingle { s: stored, a: store_base, .. },
                Instruction::Or { a: 3, s: receiver, b },
                Instruction::BranchAndLink { .. },
            ] if loaded == stored && store_base == receiver && b == receiver)
        }) {
            self.move_instruction_before(start + 2, start + 1);
        }
    }

    pub(super) fn move_instruction_before(&mut self, from: usize, to: usize) {
        crate::move_instruction_before_retargeting(self, from, to);
    }

    pub(super) fn plans_structured_float_store_guard_swap(
        &self,
        statement: &Statement,
        next_statement: Option<&Statement>,
    ) -> bool {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return false;
        }
        let Statement::Store {
            target:
                Expression::Member {
                    member_type,
                    ..
                },
            value: Expression::Call { name, .. },
        } = statement
        else {
            return false;
        };
        if !matches!(member_type, Type::Float | Type::Double)
            || self.call_return_types.get(name) != Some(member_type)
        {
            return false;
        }
        let Some(guard_name) = next_statement.and_then(leading_register_truth_test) else {
            return false;
        };
        self.locations
            .get(guard_name)
            .is_some_and(|location| location.class == ValueClass::General)
    }

    pub(super) fn swap_structured_float_store_with_guard_test(
        &mut self,
        store_index: usize,
    ) -> Compilation<()> {
        let Some([store, test]) = self
            .output
            .instructions
            .get(store_index..store_index.saturating_add(2))
        else {
            return Err(Diagnostic::error(
                "structured float-store schedule did not emit an adjacent guard test",
            ));
        };
        let call_result = Eabi::float_result().number;
        let is_call_result_store = matches!(
            store,
            Instruction::StoreFloatSingle { s, .. }
                | Instruction::StoreFloatDouble { s, .. }
                if *s == call_result
        );
        let is_zero_test = matches!(
            test,
            Instruction::CompareWordImmediate { immediate: 0, .. }
                | Instruction::CompareLogicalWordImmediate { immediate: 0, .. }
        );
        if !is_call_result_store || !is_zero_test {
            return Err(Diagnostic::error(
                "structured float-store schedule emitted an unexpected instruction pair",
            ));
        }
        self.output.instructions.swap(store_index, store_index + 1);
        Ok(())
    }
}

fn scaled_global_allocation_publication_start(
    instructions: &[Instruction],
) -> Option<usize> {
    instructions.windows(7).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchAndLink { .. },
                Instruction::StoreWord {
                    s: 3,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 0,
                    offset: 0,
                },
                Instruction::ShiftLeftImmediate {
                    a: 3,
                    s: 0,
                    shift: 2,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

fn indexed_allocation_pair_start(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: table,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWordIndexed {
                    s: 3,
                    a: table_base,
                    ..
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate { d: 3, a: 0, .. },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ] if table == table_base
        )
    })
}

fn pointer_table_index_cursor_publication_start(
    instructions: &[Instruction],
) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: table,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWordIndexed {
                    s: 3,
                    a: table_base,
                    b: cursor,
                },
                Instruction::AddImmediate {
                    d: induction,
                    a: induction_source,
                    immediate: 1,
                },
                Instruction::AddImmediate {
                    d: advanced_cursor,
                    a: cursor_source,
                    immediate: 4,
                },
            ] if table == table_base
                && induction == induction_source
                && advanced_cursor == cursor
                && cursor_source == cursor
                && induction != table
                && induction != cursor
                && induction != &3
        )
    })
}

fn post_call_zero_global_publication_start(
    output: &MachineFunction,
) -> Option<usize> {
    output.instructions.windows(5).enumerate().find_map(|(call, window)| {
        let [
            Instruction::BranchAndLink { .. }
            | Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
        ] = window
        else {
            return None;
        };
        if matches!(
            window[0],
            Instruction::BranchConditionalForward { target, .. }
                if target >= call
        ) {
            return None;
        }
        let first_store = direct_sda_target(output, call + 2)?;
        let load = direct_sda_target(output, call + 3)?;
        let second_store = direct_sda_target(output, call + 4)?;
        if first_store == load
            || load == second_store
            || output.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if (call + 1..=call + 4).contains(target)
                )
            })
        {
            return None;
        }
        Some(call + 1)
    })
}

fn direct_sda_target(
    output: &MachineFunction,
    instruction_index: usize,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index
            || relocation.kind != RelocationKind::EmbSda21
        {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(name)
            | RelocationTarget::ExternalWithAddend(name, _) => Some(name.as_str()),
            _ => None,
        }
    })
}

fn post_call_jump_state_reset_start(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(10).position(|window| {
        matches!(window, [
            Instruction::AddImmediate { d: first, a: 0, immediate: 1 },
            Instruction::StoreWord { s: first_store, a: owner, .. },
            Instruction::LoadFloatSingle { d: loaded_first_zero, .. },
            Instruction::StoreFloatSingle { s: stored_first_zero, a: first_float_base, .. },
            Instruction::LoadFloatSingle { d: loaded_second_zero, .. },
            Instruction::StoreFloatSingle { s: stored_second_zero, a: second_float_base, .. },
            Instruction::LoadWord { d: narrow, a: attributes, .. },
            Instruction::StoreByte { s: narrow_store, a: narrow_base, .. },
            Instruction::AddImmediate { d: lock, a: 0, immediate: 5 },
            Instruction::StoreWord { s: lock_store, a: lock_base, .. },
        ] if first == first_store
            && loaded_first_zero == stored_first_zero
            && loaded_second_zero == stored_second_zero
            && stored_first_zero == stored_second_zero
            && owner == first_float_base
            && first_float_base == second_float_base
            && second_float_base == narrow_base
            && narrow == narrow_store
            && narrow == lock
            && lock == lock_store
            && narrow_base == lock_base
            && attributes != owner)
    })
}

fn guarded_saved_receiver_float_call_start(
    instructions: &[Instruction],
) -> Option<usize> {
    instructions.windows(6).position(|window| {
        matches!(window, [
            Instruction::Branch { .. },
            Instruction::Or { a: 3, s: saved, b },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::LoadFloatSingle { d: 1, .. },
            Instruction::BranchAndLink { .. },
            _,
        ] if saved == b)
    })
}

fn leading_register_truth_test(statement: &Statement) -> Option<&str> {
    let condition = match statement {
        Statement::Expression(Expression::Conditional { condition, .. }) => condition.as_ref(),
        Statement::If { condition, .. } => condition,
        _ => return None,
    };
    truth_test_variable(condition)
}

fn truth_test_variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => match operand.as_ref() {
            Expression::Variable(name) => Some(name),
            _ => None,
        },
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (Expression::Variable(name), Expression::IntegerLiteral(0))
            | (Expression::IntegerLiteral(0), Expression::Variable(name)) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_indexed_allocation_pair_with_a_virtual_table_home() {
        let instructions = [
            Instruction::LoadWord {
                d: 40,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWordIndexed { s: 3, a: 40, b: 31 },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(3, 32),
            Instruction::load_immediate(5, 0),
            Instruction::BranchAndLink {
                target: "allocate".into(),
            },
        ];

        assert_eq!(indexed_allocation_pair_start(&instructions), Some(0));
    }

    #[test]
    fn recognizes_an_independent_pointer_table_induction_increment() {
        let instructions = [
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWordIndexed { s: 3, a: 4, b: 31 },
            Instruction::AddImmediate {
                d: 30,
                a: 30,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 31,
                immediate: 4,
            },
        ];

        assert_eq!(
            pointer_table_index_cursor_publication_start(&instructions),
            Some(0)
        );
    }

    #[test]
    fn recognizes_a_post_call_zero_and_distinct_global_copy() {
        let mut output = MachineFunction::new("publish");
        output.instructions = vec![
            Instruction::BranchAndLink {
                target: "update".into(),
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
        ];
        for (instruction_index, target) in
            [(2, "zero"), (3, "source"), (4, "destination")]
        {
            output.relocations.push(Relocation {
                instruction_index,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External(target.into()),
            });
        }

        assert_eq!(post_call_zero_global_publication_start(&output), Some(1));
    }

    #[test]
    fn recognizes_a_guarded_saved_receiver_mixed_call() {
        let instructions = [
            Instruction::Branch { target: 8 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::load_immediate(4, 0),
            Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 32,
            },
        ];

        assert_eq!(
            guarded_saved_receiver_float_call_start(&instructions),
            Some(0)
        );
    }

    #[test]
    fn recognizes_conditional_expression_truth_test() {
        let statement = Statement::Expression(Expression::Conditional {
            condition: Box::new(Expression::Variable("object".into())),
            when_true: Box::new(Expression::IntegerLiteral(0)),
            when_false: Box::new(Expression::IntegerLiteral(1)),
            origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
        });
        assert_eq!(leading_register_truth_test(&statement), Some("object"));
    }

    #[test]
    fn rejects_memory_backed_guard() {
        let statement = Statement::If {
            condition: Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset: 0,
                member_type: Type::Int,
                index_stride: None,
            },
            then_body: Vec::new(),
            else_body: Vec::new(),
        };
        assert_eq!(leading_register_truth_test(&statement), None);
    }

    #[test]
    fn recognizes_a_post_call_mixed_jump_state_reset() {
        let instructions = [
            Instruction::load_immediate(0, 1),
            Instruction::StoreWord {
                s: 0,
                a: 30,
                offset: 224,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 30,
                offset: 236,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 30,
                offset: 120,
            },
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 88,
            },
            Instruction::StoreByte {
                s: 0,
                a: 30,
                offset: 6504,
            },
            Instruction::load_immediate(0, 5),
            Instruction::StoreWord {
                s: 0,
                a: 30,
                offset: 2188,
            },
        ];

        assert_eq!(post_call_jump_state_reset_start(&instructions), Some(0));
    }
}
