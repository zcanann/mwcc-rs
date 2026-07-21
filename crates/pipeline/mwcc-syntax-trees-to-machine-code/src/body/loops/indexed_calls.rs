//! Counted global-array walks whose element address feeds several calls.
//!
//! The element cursor and counter are loop-carried values. Keeping the family
//! here avoids rediscovering/recomputing `&array[i]` independently at every call
//! site and lets the allocator color both survivors as one region.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower `for (i=0; i<N; i++) { f(&a[i]); g(&a[i]); ... }` for the legacy
    /// linkage-first compilers. The address is formed once, advanced by the
    /// element size after every iteration, and survives each call in its own
    /// callee-saved home.
    pub(crate) fn try_indexed_call_sequence_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !function.parameters.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let returns_zero = function.return_type == Type::Int
            && matches!(
                function.return_expression,
                Some(Expression::IntegerLiteral(0))
            );
        if !returns_zero {
            return Ok(false);
        }
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.declared_type != Type::Int
            || counter.initializer.is_some()
            || counter.array_length.is_some()
            || counter.is_static
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(initializer,
            Expression::Assign { target, value }
                if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                    && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
            || !matches!(step,
                Expression::Assign { target, value }
                    if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                        && matches!(value.as_ref(), Expression::Binary {
                            operator: BinaryOperator::Add,
                            left,
                            right,
                        } if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name)
                            && matches!(right.as_ref(), Expression::IntegerLiteral(1))))
        {
            return Ok(false);
        }
        let bound = match condition {
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) => {
                match right.as_ref() {
                    Expression::IntegerLiteral(value) if (1..=i16::MAX as i64).contains(value) => {
                        *value as i16
                    }
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        if body.len() < 2 {
            return Ok(false);
        }

        let mut calls = Vec::with_capacity(body.len());
        let mut array_name: Option<&str> = None;
        for statement in body {
            let Statement::Expression(Expression::Call { name, arguments }) = statement else {
                return Ok(false);
            };
            let [first, rest @ ..] = arguments.as_slice() else {
                return Ok(false);
            };
            let Expression::AddressOf { operand } = first else {
                return Ok(false);
            };
            let Expression::Index { base, index } = operand.as_ref() else {
                return Ok(false);
            };
            let (Expression::Variable(array), Expression::Variable(index)) =
                (base.as_ref(), index.as_ref())
            else {
                return Ok(false);
            };
            if index != &counter.name
                || array_name.is_some_and(|expected| expected != array)
                || rest.len() > 1
                || rest
                    .first()
                    .is_some_and(|argument| constant_value(argument).is_none())
            {
                return Ok(false);
            }
            array_name = Some(array);
            calls.push((
                name.as_str(),
                rest.first().and_then(|value| constant_value(value)),
            ));
        }
        let Some(array) = array_name else {
            return Ok(false);
        };
        let element_size = match self.globals.get(array) {
            Some(Type::Struct { size, .. }) if *size <= i16::MAX as u32 => *size as i16,
            _ => return Ok(false),
        };
        if element_size == 0
            || calls
                .iter()
                .any(|(_, constant)| constant.is_some_and(|value| i16::try_from(value).is_err()))
        {
            return Ok(false);
        }

        let element = self.fresh_virtual_general();
        let index = self.fresh_virtual_general();
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![element, index];
        self.legacy_callee_saved_frame_layout = LegacyCalleeSavedFrameLayout::PreserveLogicalSize;
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump = 5;

        // Final linkage-first prologue schedule. Virtual save operands are
        // allocated together with their body uses, yielding r31/r30.
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: element,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: index,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: index,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 4,
                a: index,
                immediate: element_size,
            });
        self.output.instructions.push(Instruction::Add {
            d: element,
            a: 0,
            b: 4,
        });

        let loop_top = self.output.instructions.len();
        for (callee, constant) in calls {
            if constant.is_some() {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: element,
                    immediate: 0,
                });
            } else {
                self.output
                    .instructions
                    .push(Instruction::move_register(3, element));
            }
            if let Some(value) = constant {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: value as i16,
                });
            }
            self.record_relocation(RelocationKind::Rel24, callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: callee.to_string(),
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: index,
            a: index,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: element,
            a: element,
            immediate: element_size,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: index,
                immediate: bound,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_top,
            });

        self.output.instructions.push(Instruction::LoadWord {
            d: element,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: index,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
