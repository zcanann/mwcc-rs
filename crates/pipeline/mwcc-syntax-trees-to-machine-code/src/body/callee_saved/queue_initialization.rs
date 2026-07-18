//! Guarded initialization of queue state around callback registration.

use super::fixed_rmw_recognize::peel_casts;
#[allow(unused_imports)]
use super::*;

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(peel_casts(expression)) == Some(expected)
}

fn equality_global(expression: &Expression, expected: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), other) if is_constant(other, expected) => Some(name),
        (other, Expression::Variable(name)) if is_constant(other, expected) => Some(name),
        _ => None,
    }
}

fn constant_global_store(statement: &Statement, expected: i64) -> Option<&str> {
    match statement {
        Statement::Store {
            target: Expression::Variable(name),
            value,
        } if is_constant(value, expected) => Some(name),
        _ => None,
    }
}

impl Generator {
    /// Emit the common SDK queue initializer: return when already initialized,
    /// clear both queue heads, register a handler, clear pending/callback state,
    /// and commit the initialized flag last.
    pub(crate) fn try_guarded_queue_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [guard, queue_clear, chunk_store, register, pending_hi_store, pending_lo_store, callback_hi_store, callback_lo_store, initialized_store] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = guard
        else {
            return Ok(false);
        };
        if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(None)]) {
            return Ok(false);
        }
        let Some(initialized) = equality_global(condition, 1) else {
            return Ok(false);
        };
        let Statement::Store {
            target: Expression::Variable(queue_hi),
            value:
                Expression::Assign {
                    target,
                    value: queue_zero,
                },
        } = queue_clear
        else {
            return Ok(false);
        };
        let Expression::Variable(queue_lo) = target.as_ref() else {
            return Ok(false);
        };
        if !is_constant(queue_zero, 0) {
            return Ok(false);
        }
        let Statement::Store {
            target: Expression::Variable(chunk_global),
            value: chunk_value,
        } = chunk_store
        else {
            return Ok(false);
        };
        let Some(chunk) = constant_value(chunk_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Statement::Expression(Expression::Call {
            name: registrar,
            arguments,
        }) = register
        else {
            return Ok(false);
        };
        let [Expression::AddressOf { operand }] = arguments.as_slice() else {
            return Ok(false);
        };
        let Expression::Variable(handler) = operand.as_ref() else {
            return Ok(false);
        };
        let Some(pending_hi) = constant_global_store(pending_hi_store, 0) else {
            return Ok(false);
        };
        let Some(pending_lo) = constant_global_store(pending_lo_store, 0) else {
            return Ok(false);
        };
        let Some(callback_hi) = constant_global_store(callback_hi_store, 0) else {
            return Ok(false);
        };
        let Some(callback_lo) = constant_global_store(callback_lo_store, 0) else {
            return Ok(false);
        };
        if constant_global_store(initialized_store, 1) != Some(initialized) {
            return Ok(false);
        }
        let names = [
            initialized,
            queue_hi,
            queue_lo.as_str(),
            chunk_global,
            pending_hi,
            pending_lo,
            callback_hi,
            callback_lo,
        ];
        if names.iter().any(|name| !self.globals.contains_key(*name)) {
            return Ok(false);
        }
        let mut distinct = names;
        distinct.sort_unstable();
        if distinct.windows(2).any(|pair| pair[0] == pair[1]) {
            return Ok(false);
        }

        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.emit_plain_nonleaf_prologue();
            }
            FrameConvention::LinkageFirst => self.emit_linkage_first_nonleaf_prologue(&[31]),
        }
        self.emit_global_load(initialized, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        let end = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, end);

        let zero_register = match self.behavior.frame_convention {
            FrameConvention::Predecrement => 4,
            FrameConvention::LinkageFirst => 31,
        };
        self.output
            .instructions
            .push(Instruction::load_immediate(zero_register, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(0, chunk));
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.record_relocation(RelocationKind::Addr16Ha, handler);
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(3, 0));
                self.record_relocation(RelocationKind::EmbSda21, queue_lo);
                self.output.instructions.push(Instruction::StoreWord {
                    s: zero_register,
                    a: 0,
                    offset: 0,
                });
                self.record_relocation(RelocationKind::Addr16Lo, handler);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0,
                });
                self.record_relocation(RelocationKind::EmbSda21, queue_hi);
                self.output.instructions.push(Instruction::StoreWord {
                    s: zero_register,
                    a: 0,
                    offset: 0,
                });
            }
            FrameConvention::LinkageFirst => {
                self.record_relocation(RelocationKind::EmbSda21, queue_lo);
                self.output.instructions.push(Instruction::StoreWord {
                    s: zero_register,
                    a: 0,
                    offset: 0,
                });
                self.record_relocation(RelocationKind::Addr16Ha, handler);
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(3, 0));
                self.record_relocation(RelocationKind::EmbSda21, queue_hi);
                self.output.instructions.push(Instruction::StoreWord {
                    s: zero_register,
                    a: 0,
                    offset: 0,
                });
                self.record_relocation(RelocationKind::Addr16Lo, handler);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0,
                });
            }
        }
        self.record_relocation(RelocationKind::EmbSda21, chunk_global);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Rel24, registrar);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: registrar.clone(),
        });

        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output
                .instructions
                .push(Instruction::load_immediate(3, 0));
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        for name in [pending_hi, pending_lo, callback_hi, callback_lo] {
            self.record_relocation(RelocationKind::EmbSda21, name);
            let register = match self.behavior.frame_convention {
                FrameConvention::Predecrement => 3,
                FrameConvention::LinkageFirst => 31,
            };
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 0,
                offset: 0,
            });
        }
        self.record_relocation(RelocationKind::EmbSda21, initialized);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(end);
        self.output.anonymous_label_bump += 2;
        self.pin_queue_helper_post_function_bump();
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.output.symbol_order = [
                initialized,
                queue_lo,
                queue_hi,
                chunk_global,
                registrar,
                pending_hi,
                pending_lo,
                callback_hi,
                callback_lo,
            ]
            .into_iter()
            .map(String::from)
            .collect();
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
