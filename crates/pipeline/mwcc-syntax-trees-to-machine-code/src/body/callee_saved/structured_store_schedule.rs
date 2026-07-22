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
    /// Hoist an independent three-register call setup across a run of three
    /// stores from one already-loaded float. MWCC fills the store issue window
    /// this way in constructor-like state initialization bodies.
    pub(super) fn schedule_structured_float_store_call_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        let Some(start) = self.output.instructions.windows(8).position(|window| {
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
        }) else {
            return;
        };
        for offset in 0..3 {
            self.move_instruction_before(start + 4 + offset, start + 1 + offset);
        }
    }

    fn move_instruction_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
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
}
