//! Two-case call-and-return switch dispatch with a shared framed epilogue.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

impl Generator {
    /// Lower the unoptimized C++ member shape
    /// `switch (short) { case A: call(this, context); return X; ... }`.
    ///
    /// The two forwarded pointer arguments survive either arm's call in r30/r31,
    /// while the narrow scrutinee receives its observable debug stack home. Both
    /// arm results and the default join one shared epilogue.
    pub(crate) fn try_switch_call_return(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [object, selector, context] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(
            object.parameter_type,
            Type::StructPointer { .. } | Type::Pointer(_)
        ) || selector.parameter_type != Type::UnsignedShort
            || !matches!(
                context.parameter_type,
                Type::StructPointer { .. } | Type::Pointer(_)
            )
        {
            return Ok(false);
        }

        // Disabled assertions survive preprocessing as `(void)0;`. They are
        // semantically empty and precede the one executable switch.
        let mut body = function.statements.as_slice();
        while matches!(
            body.first(),
            Some(Statement::Expression(Expression::Cast {
                target_type: Type::Void,
                operand,
            })) if crate::analysis::constant_value(operand).is_some()
        ) {
            body = &body[1..];
        }
        let [Statement::Switch {
            scrutinee: Expression::Variable(scrutinee),
            arms,
            default: Some(default),
        }] = body
        else {
            return Ok(false);
        };
        if scrutinee != &selector.name || arms.len() != 2 {
            return Ok(false);
        }
        let Some(default_result) = default
            .return_expression()
            .and_then(crate::analysis::constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let mut call_arms = Vec::with_capacity(2);
        for arm in arms {
            let ArmBody::Statements(statements) = &arm.body else {
                return Ok(false);
            };
            let [Statement::Expression(Expression::Call { name, arguments }), Statement::Return(Some(result))] =
                statements.as_slice()
            else {
                return Ok(false);
            };
            let Some(result) =
                crate::analysis::constant_value(result).and_then(|value| i16::try_from(value).ok())
            else {
                return Ok(false);
            };
            if arm.falls_through
                || !matches!(
                    arguments.as_slice(),
                    [Expression::Variable(first), Expression::Variable(second)]
                        if first == &object.name && second == &context.name
                )
            {
                return Ok(false);
            }
            call_arms.push((arm.value, name.as_str(), result));
        }
        let mut sorted_values = [call_arms[0].0, call_arms[1].0];
        sorted_values.sort();
        let [low, high] = sorted_values;
        if high != low + 1 || low < i16::MIN as i64 || high > i16::MAX as i64 {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31, 30];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::move_register(30, Eabi::FIRST_GENERAL_ARGUMENT),
            Instruction::StoreHalfword {
                s: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                a: 1,
                offset: 8,
            },
            Instruction::move_register(31, Eabi::FIRST_GENERAL_ARGUMENT + 2),
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 1,
                offset: 8,
            },
            Instruction::CompareWordImmediate {
                a: 0,
                immediate: high as i16,
            },
        ]);

        let high_branch = self.push_call_return_conditional(12, 2);
        let above_branch = self.push_call_return_conditional(4, 0);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: low as i16,
            });
        let low_branch = self.push_call_return_conditional(4, 0);
        let below_branch = self.push_call_return_branch();

        let mut body_starts = std::collections::HashMap::new();
        let mut join_branches = Vec::with_capacity(2);
        for (value, callee, result) in call_arms {
            body_starts.insert(value, self.output.instructions.len());
            self.output.instructions.extend([
                Instruction::move_register(Eabi::FIRST_GENERAL_ARGUMENT, 30),
                Instruction::move_register(Eabi::FIRST_GENERAL_ARGUMENT + 1, 31),
            ]);
            self.record_relocation(RelocationKind::Rel24, callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: callee.to_owned(),
            });
            self.output.instructions.push(Instruction::load_immediate(
                Eabi::general_result().number,
                result,
            ));
            join_branches.push(self.push_call_return_branch());
        }
        let default_start = self.output.instructions.len();
        self.output.instructions.push(Instruction::load_immediate(
            Eabi::general_result().number,
            default_result,
        ));
        let epilogue = self.output.instructions.len();

        self.patch_call_return_branch(high_branch, body_starts[&high]);
        self.patch_call_return_branch(above_branch, default_start);
        self.patch_call_return_branch(low_branch, body_starts[&low]);
        self.patch_call_return_branch(below_branch, default_start);
        for branch in join_branches {
            self.patch_call_return_branch(branch, epilogue);
        }
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }

    fn push_call_return_conditional(&mut self, options: u8, condition_bit: u8) -> usize {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        index
    }

    fn push_call_return_branch(&mut self) -> usize {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        index
    }

    fn patch_call_return_branch(&mut self, index: usize, destination: usize) {
        match &mut self.output.instructions[index] {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => *target = destination,
            _ => unreachable!("switch patch points at a non-branch instruction"),
        }
    }
}
