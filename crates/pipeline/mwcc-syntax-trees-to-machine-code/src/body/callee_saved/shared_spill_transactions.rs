//! Source-parameter spill images in GC/1.1p1's unoptimized transactions.
//!
//! The source parameter is written at SP+8 after saving r30 there. Reloading
//! the parameter and restoring r30 must observe the same overwritten word.

use super::*;

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn call(statement: &Statement) -> Option<(&str, &[Expression])> {
    match statement {
        Statement::Expression(Expression::Call { name, arguments }) => Some((name, arguments)),
        _ => None,
    }
}

fn word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

fn initialized_statements(function: &Function) -> Vec<Statement> {
    function
        .locals
        .iter()
        .filter_map(|local| {
            local.initializer.as_ref().map(|value| Statement::Assign {
                name: local.name.clone(),
                value: value.clone(),
            })
        })
        .chain(function.statements.iter().cloned())
        .collect()
}

impl Generator {
    /// A guarded inline context image shares its first word with a source
    /// parameter spill in this patch's O0 layout. Express the alias as memory
    /// before the ordinary frame allocator plans the image and surviving token.
    pub(crate) fn materialize_shared_spill_context(&self, function: &Function) -> Option<Function> {
        let [prior, token] = function.parameters.as_slice() else {
            return None;
        };
        let [image] = function.locals.as_slice() else {
            return None;
        };
        let Type::Struct { size, align } = image.declared_type else {
            return None;
        };
        if !self.behavior.unoptimized_shared_parameter_spills
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || image.is_static
            || image.is_volatile
            || image.initializer.is_some()
            || image.array_length.is_some()
            || size < 4
            || align != 8
            || prior.parameter_type != (Type::StructPointer { element_size: size })
            || !matches!(token.parameter_type, Type::Int | Type::UnsignedInt)
        {
            return None;
        }
        let [entry, Statement::If {
            condition,
            then_body,
            else_body,
        }, tail] = function.statements.as_slice()
        else {
            return None;
        };
        let (_, entry_arguments) = call(entry)?;
        let (_, tail_arguments) = call(tail)?;
        let token_value = |value: &Expression| {
            matches!(value,
            Expression::Binary { operator: BinaryOperator::Add, left, right }
            if variable(left) == Some(token.name.as_str()) && constant_value(right).is_some())
        };
        if !else_body.is_empty()
            || !matches!(entry_arguments, [value] if variable(value) == Some(token.name.as_str()))
            || !matches!(tail_arguments, [value] if token_value(value))
        {
            return None;
        }
        let [clear, activate, callback, reclear, restore] = then_body.as_slice() else {
            return None;
        };
        let (clear, clear_arguments) = call(clear)?;
        let (activate, activate_arguments) = call(activate)?;
        let (callback, callback_arguments) = call(callback)?;
        let (reclear, reclear_arguments) = call(reclear)?;
        let (restore, restore_arguments) = call(restore)?;
        let image_address = |value: &Expression| {
            matches!(value,
            Expression::AddressOf { operand } if variable(operand) == Some(image.name.as_str()))
        };
        let guarded_callback = match condition {
            Expression::Variable(name) => name == callback,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } => {
                (variable(left) == Some(callback) && constant_value(right) == Some(0))
                    || (variable(right) == Some(callback) && constant_value(left) == Some(0))
            }
            _ => false,
        };
        if !guarded_callback
            || !matches!(
                self.globals.get(callback),
                Some(Type::Pointer(_) | Type::StructPointer { .. })
            )
            || clear != reclear
            || activate != restore
            || !callback_arguments.is_empty()
            || !matches!(clear_arguments, [address, value] if image_address(address) && variable(value) == Some(token.name.as_str()))
            || !matches!(activate_arguments, [address] if image_address(address))
            || !matches!(reclear_arguments, [address, value] if image_address(address) && token_value(value))
            || !matches!(restore_arguments, [value] if variable(value) == Some(prior.name.as_str()))
        {
            return None;
        }
        let slot = Expression::Member {
            base: Box::new(Expression::AddressOf {
                operand: Box::new(Expression::Variable(image.name.clone())),
            }),
            offset: 0,
            member_type: prior.parameter_type,
            index_stride: None,
        };
        let mut lowered = function.clone();
        lowered.statements = std::iter::once(Statement::Store {
            target: slot.clone(),
            value: Expression::Variable(prior.name.clone()),
        })
        .chain(function.statements.iter().map(|statement| {
            super::structured_expression_visit::rewrite_statement(statement, &mut |value| {
                (variable(value) == Some(prior.name.as_str())).then(|| slot.clone())
            })
        }))
        .collect();
        Some(lowered)
    }

    pub(crate) fn try_shared_spill_transaction(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.behavior.unoptimized_shared_parameter_spills
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function
                .locals
                .iter()
                .any(|local| local.is_static || local.is_volatile || local.array_length.is_some())
        {
            return Ok(false);
        }
        let statements = initialized_statements(function);
        if self.try_shared_spill_swap(function, &statements)? {
            return Ok(true);
        }
        self.try_shared_spill_store_calls(function, &statements)
    }

    fn try_shared_spill_swap(
        &mut self,
        function: &Function,
        statements: &[Statement],
    ) -> Compilation<bool> {
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !word(parameter.parameter_type)
            || function.return_type != parameter.parameter_type
            || function.locals.len() != 2
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: old,
            value: Expression::Variable(global),
        }, Statement::Assign {
            name: token,
            value:
                Expression::Call {
                    name: enter,
                    arguments,
                },
        }, Statement::Store {
            target: Expression::Variable(written),
            value,
        }, tail] = statements
        else {
            return Ok(false);
        };
        let Some((leave, leave_arguments)) = call(tail) else {
            return Ok(false);
        };
        if global != written
            || global == &parameter.name
            || function.locals.iter().any(|local| local.name == *global)
            || self.globals.get(global) != Some(&parameter.parameter_type)
            || variable(value) != Some(parameter.name.as_str())
            || !arguments.is_empty()
            || !matches!(leave_arguments, [value] if variable(value) == Some(token.as_str()))
            || function.return_expression.as_ref().and_then(variable) != Some(old.as_str())
            || !function
                .locals
                .iter()
                .any(|local| local.name == *old && local.declared_type == parameter.parameter_type)
            || !function.locals.iter().any(|local| {
                local.name == *token && matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            })
            || self.skipped_inline_names.contains(enter)
            || self.skipped_inline_names.contains(leave)
            || !matches!(
                self.call_parameter_types.get(leave).map(Vec::as_slice),
                Some([Type::Int | Type::UnsignedInt])
            )
            || !matches!(
                self.call_return_types.get(enter),
                Some(Type::Int | Type::UnsignedInt)
            )
        {
            return Ok(false);
        }
        self.emit_linkage_first_nonleaf_prologue(&[31, 30]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.emit_global_load(global, 31)?;
        self.emit_call(enter, &[], None, false)?;
        self.output.instructions.extend([
            Instruction::move_register(30, 3),
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 8,
            },
        ]);
        self.emit_global_store(global, Pointee::UnsignedInt, 0)?;
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.emit_call(leave, &[], None, false)?;
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn try_shared_spill_store_calls(
        &mut self,
        function: &Function,
        statements: &[Statement],
    ) -> Compilation<bool> {
        let [output, input] = function.parameters.as_slice() else {
            return Ok(false);
        };
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !matches!(
                output.parameter_type,
                Type::Pointer(Pointee::Int | Pointee::UnsignedInt)
            )
            || !matches!(input.parameter_type, Type::Int | Type::UnsignedInt)
            || local.declared_type != input.parameter_type
        {
            return Ok(false);
        }
        let [Statement::Assign { name, value }, Statement::Store {
            target: Expression::Dereference { pointer },
            value: stored,
        }, first, last] = statements
        else {
            return Ok(false);
        };
        let Some((first, first_arguments)) = call(first) else {
            return Ok(false);
        };
        let Some((last, last_arguments)) = call(last) else {
            return Ok(false);
        };
        let [tail] = last_arguments else {
            return Ok(false);
        };
        let addend = |value: &Expression| match value {
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if variable(left) == Some(input.name.as_str()) => {
                constant_value(right).and_then(|n| i16::try_from(n).ok())
            }
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } if variable(left) == Some(input.name.as_str()) => constant_value(right)
                .and_then(|n| n.checked_neg())
                .and_then(|n| i16::try_from(n).ok()),
            _ => None,
        };
        let (Some(first_addend), Some(last_addend)) = (addend(value), addend(tail)) else {
            return Ok(false);
        };
        if name != &local.name
            || variable(pointer) != Some(output.name.as_str())
            || variable(stored) != Some(local.name.as_str())
            || !matches!(first_arguments, [value] if variable(value) == Some(local.name.as_str()))
            || self.skipped_inline_names.contains(first)
            || self.skipped_inline_names.contains(last)
            || [first, last].iter().any(|name| {
                !matches!(
                    self.call_parameter_types.get(*name).map(Vec::as_slice),
                    Some([Type::Int | Type::UnsignedInt])
                )
            })
        {
            return Ok(false);
        }
        self.emit_linkage_first_nonleaf_prologue(&[31, 30]);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::move_register(30, 4),
            Instruction::AddImmediate {
                d: 31,
                a: 30,
                immediate: first_addend,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 31,
                a: 3,
                offset: 0,
            },
            Instruction::move_register(3, 31),
        ]);
        self.emit_call(first, &[], None, false)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: last_addend,
        });
        self.emit_call(last, &[], None, false)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
