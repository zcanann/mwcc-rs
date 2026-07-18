//! Interrupt-protected transactions over scalar globals.
//!
//! These differ from callback swaps and fixed-address register programs: an
//! input and an old global value survive the critical-section calls while a
//! cursor global, its pointed-to slot, and a counter are updated together.

#[allow(unused_imports)]
use super::*;

fn variable_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn variable_step<'a>(
    target_name: &str,
    value: &'a Expression,
    operator: BinaryOperator,
    amount: i64,
) -> bool {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = value
    else {
        return false;
    };
    *actual == operator
        && variable_name(left) == Some(target_name)
        && constant_value(right) == Some(amount)
}

impl Generator {
    /// ```text
    /// state = enter();
    /// old = cursor;
    /// cursor += amount;
    /// *lengths = amount;
    /// lengths++;
    /// free_count--;
    /// leave(state);
    /// return old;
    /// ```
    pub(crate) fn try_interrupt_protected_allocator_bump(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.parameters.len() != 1
            || function.locals.len() != 2
            || function.return_type != Type::UnsignedInt
        {
            return Ok(false);
        }
        let amount = &function.parameters[0];
        if amount.parameter_type != Type::UnsignedInt {
            return Ok(false);
        }
        let [Statement::Assign {
            name: state_name,
            value:
                Expression::Call {
                    name: enter,
                    arguments: enter_arguments,
                },
        }, Statement::Assign {
            name: old_name,
            value: Expression::Variable(cursor_read),
        }, Statement::Store {
            target: Expression::Variable(cursor_write),
            value: cursor_value,
        }, Statement::Store {
            target: Expression::Dereference { pointer },
            value: Expression::Variable(stored_amount),
        }, Statement::Store {
            target: Expression::Variable(lengths_write),
            value: lengths_value,
        }, Statement::Store {
            target: Expression::Variable(free_write),
            value: free_value,
        }, Statement::Expression(Expression::Call {
            name: leave,
            arguments: leave_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some(lengths_read) = variable_name(pointer) else {
            return Ok(false);
        };
        let Some(old) = function.locals.iter().find(|local| &local.name == old_name) else {
            return Ok(false);
        };
        let Some(state) = function
            .locals
            .iter()
            .find(|local| &local.name == state_name)
        else {
            return Ok(false);
        };
        if old.name == state.name
            || old.declared_type != Type::UnsignedInt
            || !matches!(state.declared_type, Type::Int | Type::UnsignedInt)
            || function.locals.iter().any(|local| {
                local.array_length.is_some() || local.is_static || local.initializer.is_some()
            })
            || !enter_arguments.is_empty()
            || !matches!(leave_arguments.as_slice(), [Expression::Variable(name)] if name == &state.name)
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &old.name)
            || cursor_read != cursor_write
            || stored_amount != &amount.name
            || lengths_read != lengths_write
            || !variable_step(lengths_write, lengths_value, BinaryOperator::Add, 1)
            || !variable_step(free_write, free_value, BinaryOperator::Subtract, 1)
        {
            return Ok(false);
        }
        // `cursor += amount` has a variable right operand, unlike the constant
        // step helper above.
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: cursor_left,
            right: cursor_amount,
        } = cursor_value
        else {
            return Ok(false);
        };
        if variable_name(cursor_left) != Some(cursor_write)
            || variable_name(cursor_amount) != Some(amount.name.as_str())
            || self.globals.get(cursor_write.as_str()) != Some(&Type::UnsignedInt)
            || self.globals.get(lengths_write.as_str())
                != Some(&Type::Pointer(Pointee::UnsignedInt))
            || self.globals.get(free_write.as_str()) != Some(&Type::UnsignedInt)
        {
            return Ok(false);
        }

        let Some(amount_incoming) = self.lookup_general(&amount.name) else {
            return Ok(false);
        };
        let old_home = self.fresh_virtual_general();
        let amount_home = self.fresh_virtual_general();
        let homes = vec![old_home, amount_home];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.output.instructions.extend(plan.prologue());
        self.output
            .instructions
            .push(Instruction::move_register(amount_home, amount_incoming));
        self.emit_call(enter, enter_arguments, None, false)?;

        self.emit_global_load(cursor_read, old_home)?;
        self.emit_global_load(lengths_read, 4)?;
        self.output.instructions.push(Instruction::Add {
            d: 0,
            a: old_home,
            b: amount_home,
        });
        self.emit_global_store(cursor_write, Pointee::UnsignedInt, 0)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: amount_home,
            a: 4,
            offset: 0,
        });
        self.emit_global_load(lengths_read, 5)?;
        self.emit_global_load(free_write, 4)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -1,
        });
        self.emit_global_store(lengths_write, Pointee::UnsignedInt, 5)?;
        self.emit_global_store(free_write, Pointee::UnsignedInt, 0)?;

        self.locations.insert(
            state.name.clone(),
            Location {
                class: ValueClass::General,
                register: Eabi::general_result().number,
                signed: state.declared_type.is_signed(),
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.emit_call(leave, leave_arguments, None, false)?;
        self.output.instructions.push(Instruction::move_register(
            Eabi::general_result().number,
            old_home,
        ));
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The inverse allocator transaction, including its optional output store:
    /// decrement the length cursor, conditionally copy its word, subtract that
    /// length from the stack cursor, increment the free count, and return the
    /// reloaded stack cursor after leaving the critical section.
    pub(crate) fn try_interrupt_protected_allocator_free(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.parameters.len() != 1
            || function.locals.len() != 1
            || function.return_type != Type::UnsignedInt
        {
            return Ok(false);
        }
        let output = &function.parameters[0];
        if output.parameter_type != Type::Pointer(Pointee::UnsignedInt) {
            return Ok(false);
        }
        let [state] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(state.declared_type, Type::Int | Type::UnsignedInt)
            || state.array_length.is_some()
            || state.is_static
            || state.initializer.is_some()
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: state_name,
            value:
                Expression::Call {
                    name: enter,
                    arguments: enter_arguments,
                },
        }, Statement::Store {
            target: Expression::Variable(lengths_write),
            value: lengths_step,
        }, Statement::If {
            condition,
            then_body,
            else_body,
        }, Statement::Store {
            target: Expression::Variable(cursor_write),
            value: cursor_value,
        }, Statement::Store {
            target: Expression::Variable(free_write),
            value: free_value,
        }, Statement::Expression(Expression::Call {
            name: leave,
            arguments: leave_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Store {
            target: Expression::Dereference {
                pointer: output_pointer,
            },
            value: Expression::Dereference {
                pointer: length_pointer,
            },
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: cursor_left,
            right: cursor_right,
        } = cursor_value
        else {
            return Ok(false);
        };
        let Expression::Dereference {
            pointer: cursor_length_pointer,
        } = cursor_right.as_ref()
        else {
            return Ok(false);
        };
        if state_name != &state.name
            || !enter_arguments.is_empty()
            || !matches!(leave_arguments.as_slice(), [Expression::Variable(name)] if name == &state.name)
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == cursor_write)
            || variable_name(condition) != Some(output.name.as_str())
            || !else_body.is_empty()
            || variable_name(output_pointer) != Some(output.name.as_str())
            || variable_name(length_pointer) != Some(lengths_write.as_str())
            || variable_name(cursor_left) != Some(cursor_write.as_str())
            || variable_name(cursor_length_pointer) != Some(lengths_write.as_str())
            || !variable_step(lengths_write, lengths_step, BinaryOperator::Subtract, 1)
            || !variable_step(free_write, free_value, BinaryOperator::Add, 1)
            || self.globals.get(lengths_write.as_str())
                != Some(&Type::Pointer(Pointee::UnsignedInt))
            || self.globals.get(cursor_write.as_str()) != Some(&Type::UnsignedInt)
            || self.globals.get(free_write.as_str()) != Some(&Type::UnsignedInt)
        {
            return Ok(false);
        }

        let Some(output_incoming) = self.lookup_general(&output.name) else {
            return Ok(false);
        };
        let output_home = self.fresh_virtual_general();
        let plan = mwcc_vreg::FramePlan::sized_for(vec![output_home]);
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = vec![output_home];
        self.output.instructions.extend(plan.prologue());
        self.output
            .instructions
            .push(Instruction::move_register(output_home, output_incoming));
        self.emit_call(enter, enter_arguments, None, false)?;

        self.emit_global_load(lengths_write, 4)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: output_home,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -4,
        });
        self.emit_global_store(lengths_write, Pointee::UnsignedInt, 4)?;
        let branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: output_home,
            offset: 0,
        });
        let after_optional_store = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch]
        {
            *target = after_optional_store;
        }
        // The conditional block consumes two positions in mwcc's internal
        // anonymous-label walk. No symbols are emitted for the branch itself,
        // but the function's extab/extabindex names advance from @5/@6 to @7/@8.
        self.output.anonymous_label_bump += 2;

        self.emit_global_load(lengths_write, 5)?;
        self.emit_global_load(free_write, 4)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 1,
        });
        self.emit_global_load(cursor_write, 5)?;
        self.emit_global_store(free_write, Pointee::UnsignedInt, 0)?;
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 6, b: 5 });
        self.emit_global_store(cursor_write, Pointee::UnsignedInt, 0)?;

        self.locations.insert(
            state.name.clone(),
            Location {
                class: ValueClass::General,
                register: Eabi::general_result().number,
                signed: state.declared_type.is_signed(),
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.emit_call(leave, leave_arguments, None, false)?;
        // The return reload fills the slot between the saved-LR and saved-GPR
        // reloads; this is a distinct epilogue schedule from the shared canonical
        // helper (`lwz LR; lwz r3,cursor; lwz r31; mtlr`).
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: self.frame_size + 4,
        });
        self.emit_global_load(cursor_write, Eabi::general_result().number)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: output_home,
            a: 1,
            offset: self.frame_size - 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
