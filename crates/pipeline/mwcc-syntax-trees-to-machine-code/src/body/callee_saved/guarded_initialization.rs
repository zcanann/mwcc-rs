//! Guarded, interrupt-protected one-time initialization transactions.
//!
//! These routines combine several independently supported constructs, but mwcc
//! schedules them as one unit: an early-return guard precedes a three-home frame,
//! call-result parking is interleaved with callback setup, scalar stores are
//! reordered around a fixed-address base load, and the result global reloads
//! before the shared epilogue. Keeping the semantic recognizer here prevents
//! those cross-statement choices from leaking into generic call/store lowering.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts, rmw_parts};
#[allow(unused_imports)]
use super::*;

struct GuardedInitialization<'a> {
    stack_parameter: &'a str,
    entries_parameter: &'a str,
    initialized: &'a str,
    callback: &'a str,
    stack_pointer: &'a str,
    free_blocks: &'a str,
    block_lengths: &'a str,
    disable: &'a str,
    install_handler: &'a str,
    handler: &'a str,
    unmask: &'a str,
    check_size: &'a str,
    restore: &'a str,
    fixed_base: u32,
    fixed_index: i64,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn variable_store(statement: &Statement) -> Option<(&str, &Expression)> {
    match statement {
        Statement::Store {
            target: Expression::Variable(name),
            value,
        } => Some((name, value)),
        _ => None,
    }
}

fn direct_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    match statement {
        Statement::Expression(Expression::Call { name, arguments }) => Some((name, arguments)),
        _ => None,
    }
}

fn equality_with_constant(expression: &Expression, constant: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right) == Some(constant) {
        return variable(left);
    }
    if constant_value(left) == Some(constant) {
        return variable(right);
    }
    None
}

fn masked_variable(expression: &Expression, mask: i64) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = peel_casts(expression)
    else {
        return None;
    };
    if constant_value(right) == Some(mask) {
        return variable(left);
    }
    if constant_value(left) == Some(mask) {
        return variable(right);
    }
    None
}

fn word_pointer(global_type: Option<&Type>) -> bool {
    matches!(
        global_type,
        Some(Type::Pointer(_) | Type::StructPointer { .. })
    )
}

fn recognize<'a>(
    generator: &Generator,
    function: &'a Function,
) -> Option<GuardedInitialization<'a>> {
    if generator.behavior.global_addressing != GlobalAddressing::SmallData
        || !generator.frame_slots.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::UnsignedInt
        || function.parameters.len() != 2
        || function.locals.len() != 2
    {
        return None;
    }
    let [stack_parameter, entries_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if stack_parameter.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || entries_parameter.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [old, refresh] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(old.declared_type, Type::Int | Type::UnsignedInt)
        || refresh.declared_type != Type::UnsignedShort
        || function.locals.iter().any(|local| {
            local.array_length.is_some() || local.is_static || local.initializer.is_some()
        })
    {
        return None;
    }

    let [Statement::If {
        condition,
        then_body,
        else_body,
    }, Statement::Assign {
        name: old_name,
        value:
            Expression::Call {
                name: disable,
                arguments: disable_arguments,
            },
    }, callback_store, install_statement, unmask_statement, stack_store, free_store, blocks_store, Statement::Assign {
        name: refresh_name,
        value: refresh_value,
    }, Statement::Store {
        target: fixed_target,
        value: fixed_value,
    }, check_statement, initialized_store, restore_statement] = function.statements.as_slice()
    else {
        return None;
    };
    let [Statement::Return(Some(early_result))] = then_body.as_slice() else {
        return None;
    };
    let initialized = equality_with_constant(condition, 1)?;
    let (callback, callback_value) = variable_store(callback_store)?;
    let (stack_pointer, stack_value) = variable_store(stack_store)?;
    let (free_blocks, free_value) = variable_store(free_store)?;
    let (block_lengths, blocks_value) = variable_store(blocks_store)?;
    let (initialized_write, initialized_value) = variable_store(initialized_store)?;
    let (install_handler, install_arguments) = direct_call(install_statement)?;
    let (unmask, unmask_arguments) = direct_call(unmask_statement)?;
    let (check_size, check_arguments) = direct_call(check_statement)?;
    let (restore, restore_arguments) = direct_call(restore_statement)?;
    let [Expression::IntegerLiteral(interrupt), Expression::Variable(handler)] = install_arguments
    else {
        return None;
    };
    let [unmask_value] = unmask_arguments else {
        return None;
    };
    let [restore_value] = restore_arguments else {
        return None;
    };
    if !else_body.is_empty()
        || constant_value(early_result) != Some(0x4000)
        || old_name != &old.name
        || !disable_arguments.is_empty()
        || constant_value(peel_casts(callback_value)) != Some(0)
        || *interrupt != 6
        || constant_value(unmask_value) != Some(0x0200_0000)
        || constant_value(stack_value) != Some(0x4000)
        || variable(free_value) != Some(entries_parameter.name.as_str())
        || variable(blocks_value) != Some(stack_parameter.name.as_str())
        || refresh_name != &refresh.name
        || initialized_write != initialized
        || constant_value(initialized_value) != Some(1)
        || !check_arguments.is_empty()
        || variable(restore_value) != Some(old.name.as_str())
        || variable(function.return_expression.as_ref()?) != Some(stack_pointer)
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: refresh_load,
        right: refresh_mask,
    } = peel_casts(refresh_value)
    else {
        return None;
    };
    let refresh_load = if constant_value(refresh_mask) == Some(0xff) {
        refresh_load.as_ref()
    } else if constant_value(refresh_load) == Some(0xff) {
        refresh_mask.as_ref()
    } else {
        return None;
    };
    if !same_operand(refresh_load, fixed_target) {
        return None;
    }
    let (preserve_mask, inserted) = rmw_parts(fixed_target, fixed_value)?;
    if preserve_mask != -0x100 || masked_variable(inserted, 0xff) != Some(refresh.name.as_str()) {
        return None;
    }
    let (bank, fixed_index) = fixed_slot(fixed_target)?;
    let &(fixed_base, fixed_type) = generator.fixed_address_arrays.get(bank)?;
    if fixed_type != Type::UnsignedShort
        || !matches!(
            generator.globals.get(initialized),
            Some(Type::Int | Type::UnsignedInt)
        )
        || !word_pointer(generator.globals.get(callback))
        || generator.globals.get(stack_pointer) != Some(&Type::UnsignedInt)
        || generator.globals.get(free_blocks) != Some(&Type::UnsignedInt)
        || generator.globals.get(block_lengths) != Some(&Type::Pointer(Pointee::UnsignedInt))
        || generator.call_return_types.get(handler) != Some(&Type::Void)
    {
        return None;
    }
    let mut global_names = [
        initialized,
        callback,
        stack_pointer,
        free_blocks,
        block_lengths,
    ];
    global_names.sort_unstable();
    if global_names.windows(2).any(|pair| pair[0] == pair[1]) {
        return None;
    }

    Some(GuardedInitialization {
        stack_parameter: &stack_parameter.name,
        entries_parameter: &entries_parameter.name,
        initialized,
        callback,
        stack_pointer,
        free_blocks,
        block_lengths,
        disable,
        install_handler,
        handler,
        unmask,
        check_size,
        restore,
        fixed_base,
        fixed_index,
    })
}

impl Generator {
    /// Emit the SDK guarded-initialization schedule captured by `ARInit`.
    pub(crate) fn try_interrupt_protected_guarded_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(self, function) else {
            return Ok(false);
        };
        let Some(stack_incoming) = self.lookup_general(shape.stack_parameter) else {
            return Ok(false);
        };
        let Some(entries_incoming) = self.lookup_general(shape.entries_parameter) else {
            return Ok(false);
        };

        // Virtual-home creation order is allocation priority: the later call
        // result becomes r31, followed by the incoming entries and stack pointer
        // in r30/r29. Only the incoming values get prologue moves.
        let old_home = self.fresh_virtual_general();
        let entries_home = self.fresh_virtual_general();
        let stack_home = self.fresh_virtual_general();
        let homes = vec![old_home, entries_home, stack_home];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -plan.frame_size,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: plan.frame_size + 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: old_home,
            a: 1,
            offset: plan.frame_size - 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: entries_home,
            a: 1,
            offset: plan.frame_size - 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(entries_home, entries_incoming));
        self.output.instructions.push(Instruction::StoreWord {
            s: stack_home,
            a: 1,
            offset: plan.frame_size - 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(stack_home, stack_incoming));

        self.emit_global_load(shape.initialized, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        let initialize = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, initialize);
        self.output.instructions.push(Instruction::load_immediate(
            Eabi::general_result().number,
            0x4000,
        ));
        self.emit_branch_to(epilogue);
        self.bind_label(initialize);
        self.output.anonymous_label_bump += 2;

        self.record_relocation(RelocationKind::Rel24, shape.disable);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.disable.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::move_register(5, Eabi::general_result().number));
        self.record_relocation(RelocationKind::Addr16Ha, shape.handler);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.emit_global_store(shape.callback, Pointee::UnsignedInt, 0)?;
        self.record_relocation(RelocationKind::Addr16Lo, shape.handler);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 6));
        self.output
            .instructions
            .push(Instruction::move_register(old_home, 5));
        self.record_relocation(RelocationKind::Rel24, shape.install_handler);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.install_handler.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0x0200));
        self.record_relocation(RelocationKind::Rel24, shape.unmask);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.unmask.to_string(),
        });

        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0x4000));
        self.emit_global_store(shape.free_blocks, Pointee::UnsignedInt, entries_home)?;
        let (high, low) = crate::expressions::split_address(shape.fixed_base);
        let fixed_offset = i16::try_from(low as i64 + shape.fixed_index * 2)
            .map_err(|_| Diagnostic::error("fixed-address RMW displacement is out of range"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, high));
        self.emit_global_store(shape.stack_pointer, Pointee::UnsignedInt, 0)?;
        self.emit_global_store(shape.block_lengths, Pointee::UnsignedInt, stack_home)?;
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                3,
                4,
                fixed_offset,
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                0,
                4,
                fixed_offset,
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 3,
                s: 3,
                clear: 24,
            });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 3,
                s: 0,
                shift: 0,
                begin: 16,
                end: 23,
            });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                3,
                4,
                fixed_offset,
            )?);

        self.record_relocation(RelocationKind::Rel24, shape.check_size);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.check_size.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::move_register(
            Eabi::general_result().number,
            old_home,
        ));
        self.emit_global_store(shape.initialized, Pointee::UnsignedInt, 0)?;
        self.record_relocation(RelocationKind::Rel24, shape.restore);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.restore.to_string(),
        });
        self.emit_global_load(shape.stack_pointer, Eabi::general_result().number)?;

        self.bind_label(epilogue);
        self.output.instructions.extend(plan.epilogue());
        Ok(true)
    }
}
