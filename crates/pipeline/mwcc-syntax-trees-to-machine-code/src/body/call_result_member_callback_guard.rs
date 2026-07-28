//! Null-guarded indirect returns through a member of a direct-call result.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower:
    ///
    /// ```text
    /// Entry* entry = find(argument);
    /// if (entry == 0) return 0;
    /// return entry->callback();
    /// ```
    ///
    /// The direct-call result, null test, callback base, and indirect result all
    /// share r3. No value crosses either call, so the function needs only the
    /// ordinary LR linkage frame and a shared epilogue.
    pub(crate) fn try_call_result_member_callback_guard(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.statements.is_empty()
            || function.return_type == Type::Void
            || matches!(function.return_type, Type::Float | Type::Double)
            || self.frame_slots.values().any(|slot| slot.is_array)
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        let Some(Expression::Call {
            name: initializer,
            arguments: initializer_arguments,
        }) = local.initializer.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(
            local.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        ) {
            return Ok(false);
        }
        let [guard] = function.guards.as_slice() else {
            return Ok(false);
        };
        let null_guard = matches!(
            (&guard.condition, &guard.value),
            (
                Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left,
                    right,
                },
                Expression::IntegerLiteral(0),
            ) if matches!(
                (left.as_ref(), right.as_ref()),
                (Expression::Variable(name), Expression::IntegerLiteral(0))
                    | (Expression::IntegerLiteral(0), Expression::Variable(name))
                    if name == &local.name
            )
        );
        if !null_guard {
            return Ok(false);
        }
        let Some(Expression::CallThrough {
            target,
            arguments,
        }) = function.return_expression.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Member {
            base,
            offset,
            member_type:
                Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. },
            index_stride: None,
        } = target.as_ref()
        else {
            return Ok(false);
        };
        if !arguments.is_empty()
            || !matches!(base.as_ref(), Expression::Variable(name) if name == &local.name)
        {
            return Ok(false);
        }
        let Ok(callback_offset) = i16::try_from(*offset) else {
            return Ok(false);
        };

        self.emit_plain_nonleaf_prologue();
        self.output.anonymous_label_bump += 2;
        let result = Eabi::general_result().number;
        self.emit_call(initializer, initializer_arguments, Some(result), false)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: result,
                immediate: 0,
            });
        let branch_to_callback = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(result, 0));
        let branch_to_join = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let callback = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: result,
            offset: callback_offset,
        });
        self.emit_indirect_branch_and_link(12);
        let join = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_to_callback]
        {
            *target = callback;
        }
        if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
            *target = join;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
