//! Cross-statement scheduling for structured legacy bodies.
//!
//! A floating call result cannot be stored until the call completes. Build 163
//! fills that latency slot with an independent register truth test from the next
//! statement, then issues the store before the dependent branch. This module
//! recognizes only the dependency-safe source pair and verifies the emitted
//! adjacent instructions before exchanging them.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
