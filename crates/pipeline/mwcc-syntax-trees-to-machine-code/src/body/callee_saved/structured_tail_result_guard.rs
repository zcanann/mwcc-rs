//! Tail guards that return a value already in the ABI result register.
//!
//! A terminal `if (a && b) return result;` does not need a body block when
//! `result` is still in r3. MWCC branches around the early terms and turns the
//! final test into a conditional return. Keeping this CFG owner separate also
//! makes the result register's condition-spanning lifetime explicit.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn try_emit_structured_tail_result_guard(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        function: &Function,
    ) -> Compilation<bool> {
        let [Statement::Return(Some(value))] = then_body else {
            return Ok(false);
        };
        if matches!(
            function.return_type,
            Type::Float | Type::Double | Type::Void
        ) || contains_logical_or(condition)
        {
            return Ok(false);
        }
        let result = Eabi::general_result().number;
        if self.general_register_of_leaf(value).ok() != Some(result) {
            return Ok(false);
        }

        let previous_cache = self.begin_condition_global_cache(condition);
        let previous_float_cache = self.begin_condition_float_cache(condition);
        let inserted_result_reservation = self.reserved.insert(result);
        let emitted = (|| {
            self.preload_condition_global_cache(condition)?;
            let terms = super::structured::logical_and_terms(condition);
            let Some((last, preceding)) = terms.split_last() else {
                return Ok(());
            };
            let mut fallthrough_branches = Vec::with_capacity(preceding.len());
            for term in preceding {
                let (options, condition_bit) = self.emit_condition_test(term)?;
                fallthrough_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
            }
            let (options, condition_bit) = self.emit_condition_test(last)?;
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: options ^ 8,
                    condition_bit,
                });
            let fallthrough = self.output.instructions.len();
            for branch in fallthrough_branches {
                self.patch_forward(branch, fallthrough);
            }
            Ok(())
        })();
        if inserted_result_reservation {
            self.reserved.remove(&result);
        }
        self.restore_condition_global_cache(previous_cache);
        self.restore_condition_float_cache(previous_float_cache);
        emitted?;
        Ok(true)
    }
}

fn contains_logical_or(expression: &Expression) -> bool {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            ..
        } => true,
        Expression::Binary { left, right, .. } => {
            contains_logical_or(left) || contains_logical_or(right)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_conjunctions_from_disjunctions() {
        let leaf = |name: &str| Box::new(Expression::Variable(name.into()));
        let conjunction = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: leaf("a"),
            right: leaf("b"),
        };
        let disjunction = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: leaf("a"),
            right: leaf("b"),
        };
        assert!(!contains_logical_or(&conjunction));
        assert!(contains_logical_or(&disjunction));
    }
}
