//! Cross-statement scheduling between a pointer store and its trailing return.
//!
//! Build 163 moves the first independent comparison instruction ahead of a
//! single `*p = value` store. This pass runs on virtual instructions before
//! allocation, so the allocator observes the extended live range and selects
//! the same non-pointer temporary as mwcc.

use super::*;

impl Generator {
    /// `if (g) return E; g = S; return R;` for integer constants. The guarded
    /// return is emitted as a forward branch, while the continuation schedules
    /// the independent final return value into the constant-store latency slot:
    /// `li r0,S; li r3,R; stw r0,g; blr` on the modern pipeline.
    pub(super) fn try_guarded_global_constant_store_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [Statement::If {
            condition: Expression::Variable(condition_global),
            then_body,
            else_body,
        }, Statement::Store {
            target: Expression::Variable(store_global),
            value: stored,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Return(Some(guard_value))] = then_body.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() || condition_global != store_global {
            return Ok(false);
        }
        let Some(global_type) = self.globals.get(store_global.as_str()).copied() else {
            return Ok(false);
        };
        let Some(pointee) = pointee_of_type(global_type) else {
            return Ok(false);
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return Ok(false);
        }
        let Some(guard_constant) = constant_value(guard_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some(store_constant) = constant_value(stored)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some(return_constant) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let (options, condition_bit) =
            self.emit_condition_test(&Expression::Variable(condition_global.clone()))?;
        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: Eabi::general_result().number,
                a: 0,
                immediate: guard_constant,
            });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = continuation;
        }

        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: 0,
                immediate: store_constant,
            });
        let return_instruction = Instruction::AddImmediate {
            d: Eabi::general_result().number,
            a: 0,
            immediate: return_constant,
        };
        if self.behavior.guard_store_precedes_return_value {
            self.emit_global_store(store_global, pointee, GENERAL_SCRATCH)?;
            self.output.instructions.push(return_instruction);
        } else {
            self.output.instructions.push(return_instruction);
            self.emit_global_store(store_global, pointee, GENERAL_SCRATCH)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(super) fn schedule_legacy_single_pointer_store_return(
        &mut self,
        function: &Function,
        statements_start: usize,
        return_start: usize,
    ) {
        if self.behavior.integer_comparison_value_style
            != IntegerComparisonValueStyle::LegacyCarryChain
            || statements_start >= return_start
            || return_start >= self.output.instructions.len()
        {
            return;
        }

        let [Statement::Store {
            target: Expression::Dereference { .. },
            value: Expression::Variable(stored),
        }] = function.statements.as_slice()
        else {
            return;
        };
        let Some(Expression::Binary {
            operator,
            left,
            right,
        }) = function.return_expression.as_ref()
        else {
            return;
        };
        if !matches!(
            operator,
            BinaryOperator::Less | BinaryOperator::GreaterEqual | BinaryOperator::Equal
        ) || !is_zero_literal(right)
            || !matches!(left.as_ref(), Expression::Variable(name) if name == stored)
        {
            return;
        }

        let first = &self.output.instructions[return_start];
        let independent_preamble = match operator {
            BinaryOperator::Less | BinaryOperator::GreaterEqual => {
                matches!(
                    first,
                    Instruction::AddImmediate {
                        a: 0,
                        immediate: 0,
                        ..
                    }
                )
            }
            BinaryOperator::Equal => matches!(first, Instruction::Negate { .. }),
            _ => false,
        };
        if independent_preamble {
            let instruction = self.output.instructions.remove(return_start);
            self.output
                .instructions
                .insert(statements_start, instruction);
        }
    }
}
