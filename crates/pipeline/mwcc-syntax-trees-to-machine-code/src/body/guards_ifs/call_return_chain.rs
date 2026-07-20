//! Call-tested constant return chains with one shared non-leaf epilogue.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `if (check(...)) return C0; ... return DEFAULT;` with no parameters,
    /// locals, or preceding statements. The first call's argument setup is
    /// scheduled between `mflr` and the LR store; later calls are sequential.
    /// When the final true/default values differ by one, mwcc turns the call's
    /// truth value directly into the result with `subfic; subfe; addi`.
    pub(crate) fn try_call_condition_return_chain(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !self.frame_slots.is_empty()
            || function.guards.is_empty()
            || matches!(function.return_type, Type::Void | Type::Float | Type::Double)
        {
            return Ok(false);
        }
        let Some(default) = function.return_expression.as_ref().and_then(constant_value) else {
            return Ok(false);
        };
        if i16::try_from(default).is_err() {
            return Ok(false);
        }
        for guard in &function.guards {
            let Expression::Call { name, arguments } = &guard.condition else {
                return Ok(false);
            };
            if self.locations.contains_key(name.as_str())
                || self.globals.contains_key(name.as_str())
                || arguments.iter().any(|argument| constant_value(argument).is_none())
                || constant_value(&guard.value)
                    .and_then(|value| i16::try_from(value).ok())
                    .is_none()
            {
                return Ok(false);
            }
        }

        self.emit_call_condition_return_chain(function, default as i16)?;
        Ok(true)
    }

    fn emit_call_condition_return_chain(
        &mut self,
        function: &Function,
        default: i16,
    ) -> Compilation<()> {
        self.non_leaf = true;
        self.frame_size = 16;
        self.output.anonymous_label_bump = 2 * function.guards.len() as u32;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });

        let first = &function.guards[0];
        let Expression::Call { name, arguments } = &first.condition else {
            unreachable!("recognizer gated call conditions")
        };
        self.emit_arguments(arguments, name)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.clone(),
        });

        let mut epilogue_branches = Vec::new();
        for (index, guard) in function.guards.iter().enumerate() {
            if index != 0 {
                let Expression::Call { name, arguments } = &guard.condition else {
                    unreachable!("recognizer gated call conditions")
                };
                self.emit_call(name, arguments, None, false)?;
            }
            let true_value = constant_value(&guard.value).unwrap() as i16;
            let is_last = index + 1 == function.guards.len();
            if is_last && default.checked_sub(1) == Some(true_value) {
                self.output
                    .instructions
                    .push(Instruction::SubtractFromImmediate {
                        d: 0,
                        a: 3,
                        immediate: 0,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromExtended { d: 3, a: 0, b: 0 });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: default,
                });
                continue;
            }

            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
            let next = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 0,
                });
            self.load_integer_constant(3, i64::from(true_value));
            epilogue_branches.push(self.output.instructions.len());
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            let next_target = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[next]
            {
                *target = next_target;
            }
            if is_last {
                self.load_integer_constant(3, i64::from(default));
            }
        }

        let epilogue = self.output.instructions.len();
        for branch in epilogue_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(())
    }
}
