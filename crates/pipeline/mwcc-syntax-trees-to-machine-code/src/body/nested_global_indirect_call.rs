//! Indirect dispatch through a pointer nested in a large global aggregate.
//!
//! This owns the EABI transaction where an aggregate value must be copied to the outgoing
//! parameter area while later scalar parameters are permuted and the ninth argument spills.
//! It is deliberately separate from the small constant-only indirect-call family.

#[allow(unused_imports)]
use super::*;
use crate::expressions::split_address;

struct NestedGlobalDispatch<'a> {
    global: &'a str,
    global_member_offset: u32,
    callback_member_offset: i16,
    selector: i16,
    narrow_argument: NarrowArgument,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NarrowArgument {
    Address,
    Null,
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(candidate) if candidate == name)
}

fn address_of_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::AddressOf { operand } if variable(operand, name))
}

fn null_pointer(expression: &Expression) -> bool {
    matches!(expression,
        Expression::Cast {
            target_type: Type::Pointer(_) | Type::StructPointer { .. },
            operand,
        } if matches!(operand.as_ref(), Expression::IntegerLiteral(0)))
        || matches!(expression, Expression::IntegerLiteral(0))
}

fn classify(function: &Function) -> Option<NestedGlobalDispatch<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || !function.locals.is_empty()
    {
        return None;
    }
    let [aggregate, priority, narrow_address, context, item, argument0, argument1] =
        function.parameters.as_slice()
    else {
        return None;
    };
    if !matches!(aggregate.parameter_type, Type::Struct { size: 12, .. })
        || !matches!(priority.parameter_type, Type::Int | Type::UnsignedInt)
        || !matches!(
            narrow_address.parameter_type,
            Type::Short | Type::UnsignedShort
        )
        || !matches!(
            context.parameter_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
        || !matches!(item.parameter_type, Type::Short | Type::UnsignedShort)
        || !matches!(argument0.parameter_type, Type::Short | Type::UnsignedShort)
        || !matches!(argument1.parameter_type, Type::Short | Type::UnsignedShort)
    {
        return None;
    }
    let [Statement::Expression(Expression::CallThrough { target, arguments })] =
        function.statements.as_slice()
    else {
        return None;
    };
    let [selector, aggregate_use, null, context_use, narrow_address_use, item_use, priority_use, argument0_use, argument1_use] =
        arguments.as_slice()
    else {
        return None;
    };
    let selector = match selector {
        Expression::IntegerLiteral(value) => i16::try_from(*value).ok()?,
        _ => return None,
    };
    let narrow_argument = if address_of_variable(narrow_address_use, &narrow_address.name) {
        NarrowArgument::Address
    } else if null_pointer(narrow_address_use) {
        NarrowArgument::Null
    } else {
        return None;
    };
    if !variable(aggregate_use, &aggregate.name)
        || !null_pointer(null)
        || !variable(context_use, &context.name)
        || !variable(item_use, &item.name)
        || !variable(priority_use, &priority.name)
        || !variable(argument0_use, &argument0.name)
        || !variable(argument1_use, &argument1.name)
    {
        return None;
    }
    let Expression::Member {
        base: callback_base,
        offset: callback_member_offset,
        member_type: Type::Pointer(_) | Type::StructPointer { .. },
        index_stride: None,
    } = target.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base: global_base,
        offset: global_member_offset,
        member_type: Type::StructPointer { .. },
        index_stride: None,
    } = callback_base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = global_base.as_ref() else {
        return None;
    };
    let callback_member_offset = i16::try_from(*callback_member_offset).ok()?;
    // Only the measured large-offset form is claimed. A zero high half removes the `addis` and
    // enters a different scheduling family.
    if split_address(*global_member_offset).0 == 0 {
        return None;
    }
    Some(NestedGlobalDispatch {
        global,
        global_member_offset: *global_member_offset,
        callback_member_offset,
        selector,
        narrow_argument,
    })
}

impl Generator {
    /// Lower a bare nested-global callback with a 12-byte aggregate second argument and nine
    /// integer-class outgoing arguments. This is the measured `effect_clip->make_effect_proc`
    /// EABI shape: copy the aggregate into the caller frame, preserve the two colliding incoming
    /// registers, spill the ninth argument, then dispatch through the loaded callback.
    pub(crate) fn try_nested_global_indirect_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || !matches!(self.globals.get(plan.global), Some(Type::Struct { .. }))
        {
            return Ok(false);
        }
        for (parameter, expected_register) in function.parameters.iter().zip(3u8..=9) {
            if !matches!(self.locations.get(&parameter.name), Some(location)
                if location.class == ValueClass::General && location.register == expected_register)
            {
                return Ok(false);
            }
        }

        if plan.narrow_argument == NarrowArgument::Null {
            self.emit_null_narrow_argument_dispatch(&plan);
            return Ok(true);
        }

        let (member_high, member_low) = split_address(plan.global_member_offset);
        self.output.pre_scheduled = true;
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30];
        self.epilogue_lr_before_gprs = true;

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(10, plan.global);
        let early_aggregate_loads = self.behavior.nested_global_dispatch_schedule
            == NestedGlobalDispatchSchedule::EarlyAggregateLoads;
        if early_aggregate_loads {
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: 3,
                offset: 4,
            });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.record_relocation(RelocationKind::Addr16Lo, plan.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 10,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(10, 8));
        self.output
            .instructions
            .push(Instruction::move_register(8, 7));
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 11,
                a: 11,
                immediate: member_high,
            });
        if early_aggregate_loads {
            self.output.instructions.push(Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 0,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        if early_aggregate_loads {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 8,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 20,
        });
        if !early_aggregate_loads {
            self.output.instructions.push(Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 0,
            });
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: 3,
                offset: 4,
            });
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 8,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: plan.selector,
            });
        }
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 5,
            a: 1,
            offset: 16,
        });
        if early_aggregate_loads {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: plan.selector,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 12,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(9, 30));
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 11,
            offset: member_low,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 11,
            offset: plan.callback_member_offset,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.emit_epilogue_and_return();
        Ok(true)
    }


    /// The null fifth-argument sibling needs only the incoming priority in a
    /// callee-saved home. Its other colliding inputs can be permuted after the
    /// three-word aggregate has been loaded, producing the compact one-save
    /// schedule used by the Animal Crossing effect constructors.
    fn emit_null_narrow_argument_dispatch(&mut self, plan: &NestedGlobalDispatch<'_>) {
        let (member_high, member_low) = split_address(plan.global_member_offset);
        self.output.pre_scheduled = true;
        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![31];
        self.epilogue_lr_before_gprs = true;

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(5, plan.global);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.record_relocation(RelocationKind::Addr16Lo, plan.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 11,
                a: 5,
                immediate: member_high,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 3,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: plan.selector,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(10, 8));
        self.output
            .instructions
            .push(Instruction::move_register(8, 7));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 12,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(9, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 11,
            offset: member_low,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 11,
            offset: plan.callback_member_offset,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.emit_epilogue_and_return();
    }
}
