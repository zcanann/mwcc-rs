//! Lower value-shaped expressions whose result is discarded.

use super::*;

impl Generator {
    /// Recognize a discarded scalar read that optimization removes completely.
    ///
    /// `(void)name` still evaluates `name` in the C abstract machine, so a volatile
    /// global must retain its load. Register locals and non-volatile globals have no
    /// observable read side effect and mwcc emits no instruction for them.
    pub(crate) fn is_discarded_pure_value(&self, expression: &Expression) -> bool {
        // Decomp sources also use bare local reads as allocation hints (AX's
        // `dst;`). Reading a resident value needs no instruction. Frame-backed
        // objects, including volatile locals, retain their memory-access path.
        if let Expression::Variable(name) = expression {
            return self.locations.contains_key(name) && !self.frame_slots.contains_key(name);
        }
        let Expression::Cast {
            target_type: Type::Void,
            operand,
        } = expression
        else {
            return false;
        };
        let Expression::Variable(name) = operand.as_ref() else {
            return false;
        };
        self.locations.contains_key(name.as_str())
            || (self.globals.contains_key(name.as_str())
                && !self.volatile_globals.contains(name.as_str()))
    }

    /// Discarded integer unary operations retain operand effects. Optimized
    /// MWCC removes the operation itself; O0 still materializes it in r0.
    pub(crate) fn try_emit_discarded_integer_unary(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        let Expression::Unary { operand, .. } = expression else {
            return Ok(false);
        };
        if self.is_float_value(operand) {
            return Ok(false);
        }
        if self.behavior.optimization == mwcc_versions::Optimization::O0 {
            self.evaluate_general(expression, GENERAL_SCRATCH)?;
        } else {
            self.emit_discarded_integer_unary_operand(operand)?;
        }
        Ok(true)
    }

    fn emit_discarded_integer_unary_operand(&mut self, operand: &Expression) -> Compilation<()> {
        match operand {
            Expression::Variable(name)
                if (self.locations.contains_key(name) && !self.frame_slots.contains_key(name))
                    || (self.globals.contains_key(name)
                        && !self.volatile_globals.contains(name)) =>
            {
                Ok(())
            }
            Expression::IntegerLiteral(_) => Ok(()),
            Expression::Unary { operand, .. } => self.emit_discarded_integer_unary_operand(operand),
            Expression::Call { .. }
            | Expression::CallThrough { .. }
            | Expression::VirtualCall { .. } => {
                self.emit_statement(&Statement::Expression(operand.clone()))
            }
            // Memory and computed operands retain their ordinary evaluation.
            // In particular, a volatile access must not disappear with `!`.
            _ => self.evaluate_general(operand, GENERAL_SCRATCH),
        }
    }

    /// Lower `condition ? (void)0 : call()` (and its mirrored form) as a
    /// guarded call. Macro assertions use this expression-statement shape after
    /// preprocessing; mwcc branches over the cold call without materializing a
    /// ternary value.
    pub(crate) fn try_emit_conditional_call_statement(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        if self.try_emit_discarded_assertion(expression)?
            || self.try_emit_simple_discarded_assertion(expression)?
        {
            return Ok(true);
        }
        let Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } = expression
        else {
            return Ok(false);
        };

        let is_void_noop = |arm: &Expression| {
            matches!(
                arm,
                Expression::Cast {
                    target_type: Type::Void,
                    operand,
                } if matches!(operand.as_ref(), Expression::IntegerLiteral(_))
            )
        };
        let (call, call_when_true) = match (when_true.as_ref(), when_false.as_ref()) {
            (call @ Expression::Call { .. }, noop) if is_void_noop(noop) => (call, true),
            (noop, call @ Expression::Call { .. }) if is_void_noop(noop) => (call, false),
            _ => return Ok(false),
        };
        let Expression::Call { name, arguments } = call else {
            unreachable!("the arm matcher restricts this to a direct call")
        };

        let (skip_when_false, condition_bit) = self.emit_condition_test(condition)?;
        let skip_call = if call_when_true {
            skip_when_false
        } else {
            skip_when_false ^ 8
        };
        let end = self.fresh_label();
        self.emit_branch_conditional_to(skip_call, condition_bit, end);
        self.emit_call(name, arguments, None, false)?;
        self.bind_label(end);
        Ok(true)
    }
}
