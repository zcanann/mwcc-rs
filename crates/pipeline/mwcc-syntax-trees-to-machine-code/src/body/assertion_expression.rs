//! Discarded assertion expressions and their cold variadic report call.

use super::*;

impl Generator {
    /// Dense saved frames expose the assertion's boolean temporary and branch
    /// schedule directly. The entry owner has already copied the tested
    /// parameter with record enabled, so this emits the remaining range test
    /// and cold report diamond without materializing a second boolean value.
    pub(crate) fn try_emit_dense_frame_assertion(
        &mut self,
        expression: &Expression,
        condition_register: u8,
        zero_preloaded: bool,
    ) -> Compilation<bool> {
        let Some(shape) = DiscardedAssertion::recognize(expression) else {
            return Ok(false);
        };
        let Some((name, upper)) = dense_frame_assertion_range(&shape) else {
            return Ok(false);
        };
        if self.lookup_general(&name) != Some(condition_register) {
            return Ok(false);
        }
        if !zero_preloaded {
            self.output
                .instructions
                .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        }

        let below = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: condition_register,
                immediate: upper,
            });
        let above = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
        let boolean_join = self.output.instructions.len();
        for branch in [below, above] {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch]
            {
                *target = boolean_join;
            }
        }
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                clear: 24,
            });
        let done = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        if !self.try_emit_dense_frame_assertion_report(shape.name, shape.arguments)? {
            return Ok(false);
        }
        let after_report = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[done]
        {
            *target = after_report;
        }
        self.output.anonymous_label_bump += 5;
        Ok(true)
    }

    fn try_emit_dense_frame_assertion_report(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let [Expression::StringLiteral(file), line, Expression::StringLiteral(asserted)] =
            arguments
        else {
            return Ok(false);
        };
        let Some(line) = constant_value(line).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        if !self.variadic_callees.contains(name)
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || file.len() + 1 <= 8
            || asserted.len() + 1 <= 8
        {
            return Ok(false);
        }

        let file = self.string_literal_placeholder(file);
        let asserted = self.string_literal_placeholder(asserted);
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                self.emit_address_high(3, &file);
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterClear { d: 6 });
                self.emit_address_high(4, &asserted);
                self.emit_string_address_low(&asserted, 4, 5);
                self.emit_string_address_low(&file, 3, 3);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, line));
            }
            FrameConvention::Predecrement => {
                self.emit_address_high(3, &file);
                self.emit_address_high(5, &asserted);
                self.emit_string_address_low(&file, 3, 3);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, line));
                self.emit_string_address_low(&asserted, 5, 5);
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterClear { d: 6 });
            }
        }
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_string(),
        });
        Ok(true)
    }

    /// Lower the SDK assertion shape `(void)(condition || (report(...), 0))`.
    /// The logical value and linkage sequence are scheduled as one region;
    /// treating the call as an ordinary expression loses both MWCC schedules.
    pub(crate) fn try_emit_discarded_assertion(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        let Some(shape) = DiscardedAssertion::recognize(expression) else {
            return Ok(false);
        };
        let left = comparison_with_constant_on_right(shape.left);
        let right = comparison_with_constant_on_right(shape.right);
        let delayed_linkage = self.take_plain_assertion_linkage();

        let condition_registers = self.registers_used_by(shape.condition);
        let mut condition_registers = condition_registers.iter();
        let Some(&condition_register) = condition_registers.next() else {
            return Ok(false);
        };
        if condition_registers.next().is_some() {
            return Ok(false);
        }

        let value_register = if self.behavior.logical_or_value_style
            == mwcc_versions::LogicalOrValueStyle::TrueFirst
        {
            let result = self.free_general_excluding(condition_register)?;
            let left_start = self.output.instructions.len();
            let (left_skip, left_bit) = self.emit_condition_test(&left)?;
            if let Some(linkage) = &delayed_linkage {
                self.insert_instruction(left_start + 1, linkage.lr_store().clone());
            }
            self.output
                .instructions
                .push(Instruction::load_immediate(0, 1));
            self.output
                .instructions
                .push(Instruction::load_immediate(result, 0));
            if let Some(DelayedLinkage::LinkageFirst { frame_update, .. }) = delayed_linkage {
                self.output.instructions.push(frame_update);
            }
            let join = self.fresh_label();
            self.emit_branch_conditional_to(left_skip, left_bit, join);
            let (right_skip, right_bit) = self.emit_condition_test(&right)?;
            self.emit_branch_conditional_to(right_skip, right_bit, join);
            self.output
                .instructions
                .push(Instruction::move_register(result, 0));
            self.bind_label(join);
            result
        } else {
            let left_start = self.output.instructions.len();
            let (left_skip, left_bit) = self.emit_condition_test(&left)?;
            if let Some(DelayedLinkage::Predecrement { lr_store }) = delayed_linkage {
                self.insert_instruction(left_start + 1, lr_store);
            }
            self.output
                .instructions
                .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
            let join = self.fresh_label();
            self.emit_branch_conditional_to(left_skip, left_bit, join);
            let (right_skip, right_bit) = self.emit_condition_test(&right)?;
            self.emit_branch_conditional_to(right_skip, right_bit, join);
            self.output
                .instructions
                .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
            self.bind_label(join);
            if GENERAL_SCRATCH != 0 {
                self.output
                    .instructions
                    .push(Instruction::move_register(0, GENERAL_SCRATCH));
            }
            0
        };

        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: value_register,
                immediate: 0,
            });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, done); // bne
        if !self.try_emit_assertion_report_call(shape.name, shape.arguments)? {
            self.emit_call(shape.name, shape.arguments, None, false)?;
        }
        self.bind_label(done);
        // The two short-circuit joins and the discarded outer OR leave five
        // optimizer-only labels ahead of the assertion strings in every
        // measured PowerPC generation.
        self.output.anonymous_label_bump += 5;
        Ok(true)
    }

    /// Remove the fixed pieces which MWCC delays into the first condition's
    /// dependency gaps. This only claims an assertion at the start of a plain
    /// non-leaf body; later assertions keep the already-established frame.
    fn take_plain_assertion_linkage(&mut self) -> Option<DelayedLinkage> {
        match self.output.instructions.as_slice() {
            [Instruction::MoveFromLinkRegister { d: 0 }, lr_store @ Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            }, frame_update @ Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            }] => {
                let delayed = DelayedLinkage::LinkageFirst {
                    lr_store: lr_store.clone(),
                    frame_update: frame_update.clone(),
                };
                self.output.instructions.truncate(1);
                Some(delayed)
            }
            [Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            }, Instruction::MoveFromLinkRegister { d: 0 }, lr_store @ Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            }] => {
                let delayed = DelayedLinkage::Predecrement {
                    lr_store: lr_store.clone(),
                };
                self.output.instructions.truncate(2);
                Some(delayed)
            }
            _ => None,
        }
    }

    fn insert_instruction(&mut self, position: usize, instruction: Instruction) {
        self.output.instructions.insert(position, instruction);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= position {
                relocation.instruction_index += 1;
            }
        }
    }

    /// Match the measured `report(file, line, expression, ...)` argument shape.
    /// Other calls retain the general argument scheduler.
    fn try_emit_assertion_report_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let [Expression::StringLiteral(file), line, Expression::StringLiteral(asserted)] =
            arguments
        else {
            return Ok(false);
        };
        let Some(line) = constant_value(line).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        if !self.variadic_callees.contains(name)
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || file.len() + 1 > 8
            || asserted.len() + 1 <= 8
        {
            return Ok(false);
        }

        // Pool ordinals follow source argument order even when instruction
        // scheduling materializes the final argument first.
        self.string_literal_placeholder(file);
        let asserted = self.string_literal_placeholder(asserted);
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                self.emit_address_high(3, &asserted);
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterClear { d: 6 });
                self.emit_string_address_low(&asserted, 3, 5);
                self.emit_string_literal(file, 3)?;
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, line));
            }
            FrameConvention::Predecrement => {
                self.emit_address_high(4, &asserted);
                self.emit_string_literal(file, 3)?;
                self.emit_string_address_low(&asserted, 4, 5);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, line));
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterClear { d: 6 });
            }
        }
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_string(),
        });
        Ok(true)
    }
}

pub(crate) fn dense_frame_assertion_parameter(expression: &Expression) -> Option<String> {
    let shape = DiscardedAssertion::recognize(expression)?;
    dense_frame_assertion_range(&shape).map(|(name, _)| name)
}

fn dense_frame_assertion_range(shape: &DiscardedAssertion<'_>) -> Option<(String, i16)> {
    let left = comparison_with_constant_on_right(shape.left);
    let right = comparison_with_constant_on_right(shape.right);
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: left_value,
        right: left_constant,
    } = &left
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: right_value,
        right: right_constant,
    } = &right
    else {
        return None;
    };
    let (Expression::Variable(left_name), Expression::Variable(right_name)) =
        (left_value.as_ref(), right_value.as_ref())
    else {
        return None;
    };
    if left_name != right_name || constant_value(left_constant) != Some(0) {
        return None;
    }
    let upper = constant_value(right_constant).and_then(|value| i16::try_from(value).ok())?;
    Some((left_name.clone(), upper))
}

enum DelayedLinkage {
    LinkageFirst {
        lr_store: Instruction,
        frame_update: Instruction,
    },
    Predecrement {
        lr_store: Instruction,
    },
}

impl DelayedLinkage {
    fn lr_store(&self) -> &Instruction {
        match self {
            Self::LinkageFirst { lr_store, .. } | Self::Predecrement { lr_store } => lr_store,
        }
    }
}

struct DiscardedAssertion<'a> {
    condition: &'a Expression,
    left: &'a Expression,
    right: &'a Expression,
    name: &'a str,
    arguments: &'a [Expression],
}

impl<'a> DiscardedAssertion<'a> {
    fn recognize(expression: &'a Expression) -> Option<Self> {
        let Expression::Cast {
            target_type: Type::Void,
            operand,
        } = expression
        else {
            return None;
        };
        let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: condition,
            right: failure,
        } = operand.as_ref()
        else {
            return None;
        };
        let Expression::Comma {
            left: call,
            right: discarded,
        } = failure.as_ref()
        else {
            return None;
        };
        if constant_value(discarded) != Some(0) {
            return None;
        }
        let Expression::Call { name, arguments } = call.as_ref() else {
            return None;
        };
        let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } = condition.as_ref()
        else {
            return None;
        };
        Some(Self {
            condition,
            left,
            right,
            name,
            arguments,
        })
    }
}

fn comparison_with_constant_on_right(expression: &Expression) -> Expression {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return expression.clone();
    };
    if constant_value(left).is_none() || constant_value(right).is_some() {
        return expression.clone();
    }
    let swapped = match operator {
        BinaryOperator::Less => BinaryOperator::Greater,
        BinaryOperator::Greater => BinaryOperator::Less,
        BinaryOperator::LessEqual => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::LessEqual,
        BinaryOperator::Equal | BinaryOperator::NotEqual => *operator,
        _ => return expression.clone(),
    };
    Expression::Binary {
        operator: swapped,
        left: right.clone(),
        right: left.clone(),
    }
}
