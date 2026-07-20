//! Context-switching handlers with an optional memory-resident callback.
//!
//! The handler's large address-taken context local determines the frame, while
//! its incoming context survives four direct calls in one callee-saved home.
//! Between the paired context calls, mwcc loads the callback global once into
//! r12, tests that same value, and conditionally calls through it. This is one
//! cross-statement schedule; generic frame, fixed-RMW, and if emitters cannot
//! independently reproduce its prologue interleaves or load reuse.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts};
#[allow(unused_imports)]
use super::*;

struct ContextCallbackHandler<'a> {
    context_parameter: &'a str,
    callback: &'a str,
    clear_context: &'a str,
    set_context: &'a str,
    fixed_base: u32,
    fixed_index: i64,
    preserve_mask: i16,
    set_bits: u16,
    context_size: u16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn direct_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    match statement {
        Statement::Expression(Expression::Call { name, arguments }) => Some((name, arguments)),
        _ => None,
    }
}

fn address_of_variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::AddressOf { operand } => variable(operand),
        _ => None,
    }
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
) -> Option<ContextCallbackHandler<'a>> {
    if generator.behavior.global_addressing != GlobalAddressing::SmallData
        || !generator.frame_slots.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::Void
        || function.return_expression.is_some()
        || function.parameters.len() != 2
        || function.locals.len() != 2
    {
        return None;
    }
    let [interrupt, context] = function.parameters.as_slice() else {
        return None;
    };
    let Type::StructPointer {
        element_size: context_size,
    } = context.parameter_type
    else {
        return None;
    };
    if interrupt.parameter_type != Type::Short || context_size == 0 {
        return None;
    }
    let [local_context, temporary] = function.locals.as_slice() else {
        return None;
    };
    if local_context.declared_type
        != (Type::Struct {
            size: context_size,
            align: 8,
        })
        || temporary.declared_type != Type::UnsignedShort
        || function.locals.iter().any(|local| {
            local.array_length.is_some()
                || local.is_static
                || local.initializer.is_some()
                || local.data_bytes.is_some()
        })
    {
        return None;
    }

    let [Statement::Assign {
        name: loaded_name,
        value: loaded_value,
    }, Statement::Assign {
        name: updated_name,
        value: updated_value,
    }, Statement::Store {
        target: fixed_target,
        value: stored_value,
    }, first_clear, first_set, Statement::If {
        condition,
        then_body,
        else_body,
    }, second_clear, second_set] = function.statements.as_slice()
    else {
        return None;
    };
    if loaded_name != &temporary.name
        || updated_name != &temporary.name
        || variable(stored_value) != Some(temporary.name.as_str())
        || !else_body.is_empty()
    {
        return None;
    }
    let (bank, fixed_index) = fixed_slot(loaded_value)?;
    let (store_bank, store_index) = fixed_slot(fixed_target)?;
    if bank != store_bank || fixed_index != store_index {
        return None;
    }
    let &(fixed_base, fixed_type) = generator.fixed_address_arrays.get(bank)?;
    if fixed_type != Type::UnsignedShort {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: preserved,
        right: set_bits,
    } = peel_casts(updated_value)
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: preserved_value,
        right: preserve_mask,
    } = peel_casts(preserved)
    else {
        return None;
    };
    if variable(preserved_value) != Some(temporary.name.as_str()) {
        return None;
    }
    let preserve_mask = i16::try_from(constant_value(preserve_mask)?).ok()?;
    let set_bits = u16::try_from(constant_value(set_bits)?).ok()?;

    let (clear_context, first_clear_arguments) = direct_call(first_clear)?;
    let (set_context, first_set_arguments) = direct_call(first_set)?;
    let (second_clear_name, second_clear_arguments) = direct_call(second_clear)?;
    let (second_set_name, second_set_arguments) = direct_call(second_set)?;
    if clear_context != second_clear_name
        || set_context != second_set_name
        || !matches!(first_clear_arguments, [argument] if address_of_variable(argument) == Some(local_context.name.as_str()))
        || !matches!(first_set_arguments, [argument] if address_of_variable(argument) == Some(local_context.name.as_str()))
        || !matches!(second_clear_arguments, [argument] if address_of_variable(argument) == Some(local_context.name.as_str()))
        || !matches!(second_set_arguments, [argument] if variable(argument) == Some(context.name.as_str()))
    {
        return None;
    }
    let callback = variable(condition)?;
    let [Statement::Expression(Expression::Call {
        name: callback_call,
        arguments: callback_arguments,
    })] = then_body.as_slice()
    else {
        return None;
    };
    if callback_call != callback
        || !callback_arguments.is_empty()
        || !word_pointer(generator.globals.get(callback))
    {
        return None;
    }

    Some(ContextCallbackHandler {
        context_parameter: &context.name,
        callback,
        clear_context,
        set_context,
        fixed_base,
        fixed_index,
        preserve_mask,
        set_bits,
        context_size: u16::try_from(context_size).ok()?,
    })
}

impl Generator {
    /// Emit the context-frame optional-callback schedule captured by the SDK's
    /// ARAM interrupt handler.
    pub(crate) fn try_context_callback_handler(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(self, function) else {
            return Ok(false);
        };
        let Some(context_incoming) = self.lookup_general(shape.context_parameter) else {
            return Ok(false);
        };
        let context_home = self.fresh_virtual_general();
        let frame_size_i32 = 8i32 + i32::from(shape.context_size) + 4;
        let frame_size = i16::try_from((frame_size_i32 + 15) / 16 * 16)
            .map_err(|_| Diagnostic::error("context-handler frame is too large"))?;
        let address = shape.fixed_base;
        let (high, low) = crate::expressions::split_address(address);
        let linkage_first = self.behavior.frame_convention == FrameConvention::LinkageFirst;
        let fixed_offset =
            i16::try_from(if linkage_first { 0 } else { low as i64 } + shape.fixed_index * 2)
                .map_err(|_| {
                    Diagnostic::error("fixed-address handler displacement is out of range")
                })?;
        let local_offset = if linkage_first { 16 } else { 8 };

        self.non_leaf = true;
        self.frame_size = frame_size;
        self.callee_saved = vec![context_home];
        let fixed_base = if linkage_first { 3 } else { 6 };
        if linkage_first {
            // This owner already knows its physical frame. Build 163 fills both
            // linkage hazards with the fixed-register address and mask before
            // updating SP, then places the 8-byte-aligned context at sp+16.
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(fixed_base, high));
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: fixed_base,
                a: fixed_base,
                immediate: low,
            });
            self.output
                .instructions
                .push(Instruction::load_immediate(0, shape.preserve_mask));
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -frame_size,
                });
            self.output.instructions.push(Instruction::StoreWord {
                s: context_home,
                a: 1,
                offset: frame_size - 4,
            });
            self.emit_callee_saved_home_copy(context_home, context_incoming);
        } else {
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -frame_size,
                });
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(fixed_base, high));
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: frame_size + 4,
            });
            self.output
                .instructions
                .push(Instruction::load_immediate(0, shape.preserve_mask));
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: local_offset,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: context_home,
                a: 1,
                offset: frame_size - 4,
            });
            self.output
                .instructions
                .push(Instruction::move_register(context_home, context_incoming));
        }
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                5,
                fixed_base,
                fixed_offset,
            )?);
        if !linkage_first {
            self.output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: 4,
                    s: 5,
                    clear: 16,
                });
        }
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: if linkage_first { 5 } else { 4 },
            b: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: shape.set_bits,
        });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                fixed_base,
                fixed_offset,
            )?);

        if linkage_first {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: local_offset,
            });
        }

        self.record_relocation(RelocationKind::Rel24, shape.clear_context);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.clear_context.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: local_offset,
        });
        self.record_relocation(RelocationKind::Rel24, shape.set_context);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.set_context.to_string(),
        });

        self.emit_global_load(shape.callback, 12)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        let after_callback = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, after_callback);
        self.emit_indirect_branch_and_link(12);
        self.bind_label(after_callback);
        // The condition plus indirect-call control flow consumes one additional
        // internal slot in build 163's LR-dispatch path compared with the CTR path.
        self.output.anonymous_label_bump += if linkage_first { 5 } else { 4 };

        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: local_offset,
        });
        self.record_relocation(RelocationKind::Rel24, shape.clear_context);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.clear_context.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, context_home));
        self.record_relocation(RelocationKind::Rel24, shape.set_context);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.set_context.to_string(),
        });

        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: frame_size + 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: context_home,
            a: 1,
            offset: frame_size - 4,
        });
        if linkage_first {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: frame_size,
            });
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        } else {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: frame_size,
            });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
