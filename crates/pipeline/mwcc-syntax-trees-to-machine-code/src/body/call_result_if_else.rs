//! Non-leaf call-result diamonds.
//!
//! A call result may remain in `r3` while it selects between two call arms,
//! provided every arm consumes its last use before making its call. That is
//! narrower than general cross-call liveness and does not require a
//! callee-saved home.

#[allow(unused_imports)]
use super::*;

impl Generator {
    fn emit_call_result_arm(
        &mut self,
        statement: &Statement,
        result_register: u8,
    ) -> Compilation<()> {
        let Statement::Expression(Expression::Call { name, arguments }) = statement else {
            unreachable!("the owner validates both arms");
        };
        let endangered_tail = arguments.len() == 3
            && self.registers_used_by(&arguments[2]).contains(&result_register)
            && !self.registers_used_by(&arguments[0]).contains(&result_register);
        if !endangered_tail {
            return self.emit_statement(statement);
        }

        // Preserve the final computed word in its ABI destination before the
        // first argument overwrites r3. This is the general dependency that the
        // ordinary left-to-right argument emitter rejects; restricting it to a
        // three-word direct call leaves no intervening value that can be
        // clobbered by the early materialization.
        let tail_register = Eabi::FIRST_GENERAL_ARGUMENT + 2;
        self.evaluate_general(&arguments[2], tail_register)?;
        let temporary = "@call-result-if-else-tail".to_string();
        self.locations.insert(
            temporary.clone(),
            Location {
                class: ValueClass::General,
                register: tail_register,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        let mut scheduled = arguments.clone();
        scheduled[2] = Expression::Variable(temporary.clone());
        let emitted = self.emit_call(name, &scheduled, None, false);
        self.locations.remove(&temporary);
        emitted
    }

    /// Lower:
    ///
    /// ```text
    /// T discriminator = get();
    /// if (discriminator OP constant) {
    ///     then_call(...discriminator...);
    /// } else {
    ///     else_call(...discriminator...);
    /// }
    /// return stable_value;
    /// ```
    ///
    /// Both arms contain exactly one direct call, so any use of the
    /// discriminator is necessarily consumed while preparing that call. The
    /// join therefore needs only the ordinary LR linkage frame.
    pub(crate) fn try_call_result_if_else(&mut self, function: &Function) -> Compilation<bool> {
        if !function.parameters.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_none()
        {
            return Ok(false);
        }
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let ([Statement::Expression(Expression::Call { .. })], [Statement::Expression(
            Expression::Call { .. },
        )]) = (then_body.as_slice(), else_body.as_slice())
        else {
            return Ok(false);
        };

        let mut initialized = function.locals.iter().filter(|local| {
            !local.is_static
                && local.array_length.is_none()
                && matches!(local.initializer, Some(Expression::Call { .. }))
        });
        let Some(discriminator) = initialized.next() else {
            return Ok(false);
        };
        if initialized.next().is_some()
            || function
                .locals
                .iter()
                .any(|local| !local.is_static && local.name != discriminator.name)
            || expression_reads_name(
                function.return_expression.as_ref().expect("checked above"),
                &discriminator.name,
            )
        {
            return Ok(false);
        }
        let Some((_, compared)) = guard_comparison_key(condition) else {
            return Ok(false);
        };
        if !matches!(
            condition,
            Expression::Binary { left, right, .. }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &discriminator.name)
                    || matches!(right.as_ref(), Expression::Variable(name) if name == &discriminator.name)
        ) || i16::try_from(compared).is_err()
        {
            return Ok(false);
        }
        let Some(Expression::Call {
            name: initializer_name,
            arguments: initializer_arguments,
        }) = discriminator.initializer.as_ref()
        else {
            unreachable!("filtered above");
        };
        if !initializer_arguments.is_empty()
            || class_of(discriminator.declared_type)? != ValueClass::General
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 16;
        self.output.anonymous_label_bump = 3;
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_call(initializer_name, initializer_arguments, None, false)?;

        let result = Eabi::general_result().number;
        self.locations.insert(
            discriminator.name.clone(),
            Location {
                class: ValueClass::General,
                register: result,
                signed: self.signed_of(discriminator.declared_type),
                width: discriminator.declared_type.width(),
                pointee: None,
                stride: None,
            },
        );
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_to_else = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.emit_call_result_arm(&then_body[0], result)?;
        let branch_to_join = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_to_else]
        {
            *target = else_label;
        }
        self.emit_call_result_arm(&else_body[0], result)?;
        let join_label = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
            *target = join_label;
        }

        // The return value is independent of the arm calls. MWCC issues the LR
        // reload first, then materializes it into r3 at the join.
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.evaluate_tail(
            function.return_expression.as_ref().expect("checked above"),
            function.return_type,
            result,
        )?;
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
        Ok(true)
    }
}
