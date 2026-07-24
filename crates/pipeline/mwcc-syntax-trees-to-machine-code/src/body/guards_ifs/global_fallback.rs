//! Leaf global-pointer fallback getters.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower `if (primary) return primary; return fallback;` for a word-sized
    /// global, or a member loaded through the result-register parameter. MWCC
    /// keeps the first load live in r3 and turns the successful arm into
    /// `bnelr`, avoiding a duplicate reload.
    pub(crate) fn try_global_pointer_fallback_getter(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.locals.is_empty()
            || !function.statements.is_empty()
            || function.guards.len() != 1
            || !matches!(function.return_type, Type::Pointer(_) | Type::StructPointer { .. })
        {
            return Ok(false);
        }
        let guard = &function.guards[0];
        let (condition, guard_value, Some(Expression::Variable(fallback))) = (
            &guard.condition,
            &guard.value,
            function.return_expression.as_ref(),
        )
        else {
            return Ok(false);
        };
        if !same_operand(condition, guard_value)
            || !matches!(
                self.globals.get(fallback),
                Some(Type::Pointer(_) | Type::StructPointer { .. })
            )
        {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        match condition {
            Expression::Variable(primary)
                if matches!(
                    self.globals.get(primary),
                    Some(Type::Pointer(_) | Type::StructPointer { .. })
                ) =>
            {
                self.emit_global_load_value(primary, result)?;
            }
            Expression::Member {
                base,
                member_type: Type::Pointer(_) | Type::StructPointer { .. },
                ..
            } if matches!(base.as_ref(), Expression::Variable(name)
                if self.lookup_general(name) == Some(result)) =>
            {
                self.evaluate_tail(condition, function.return_type, result)?;
            }
            _ => return Ok(false),
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: result,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });
        self.emit_global_load_value(fallback, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
