//! Terminal call forwarding and whole-file IPA sibling-call elimination.

use super::*;

impl Generator {
    /// Lower the pre-sibling-call generation's simplest terminal forwarding
    /// wrapper: every entry parameter is passed unchanged, in order, to one
    /// direct call whose result is returned unchanged.
    ///
    /// This topology has no value live across the call. Keeping it here beside
    /// sibling-call lowering prevents the broad callee-saved recognizers from
    /// mistaking an opaque struct-pointer argument for a survivor and growing a
    /// spurious save slot.
    pub(crate) fn try_non_tail_call_forward(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.tail_call_optimization
            || self.behavior.terminal_indirect_tail_call
            || self.variadic_definition
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !function.statements.is_empty()
        {
            return Ok(false);
        }
        let Some(Expression::Call { name, arguments }) = function.return_expression.as_ref()
        else {
            return Ok(false);
        };
        let single_general = |value_type: Type| {
            matches!(
                value_type,
                Type::Int
                    | Type::UnsignedInt
                    | Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Pointer(_)
                    | Type::StructPointer { .. }
            )
        };
        let parameter_types_match = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() == function.parameters.len()
                && types
                    .iter()
                    .zip(&function.parameters)
                    .all(|(callee, caller)| *callee == caller.parameter_type)
        });
        if self.locations.contains_key(name)
            || self.globals.contains_key(name)
            || self.variadic_callees.contains(name)
            || !single_general(function.return_type)
            || self.call_return_types.get(name) != Some(&function.return_type)
            || !parameter_types_match
            || arguments.len() != function.parameters.len()
            || !arguments
                .iter()
                .zip(&function.parameters)
                .all(|(argument, parameter)| {
                    matches!(argument, Expression::Variable(argument_name)
                        if argument_name == &parameter.name)
                        && single_general(parameter.parameter_type)
                })
        {
            return Ok(false);
        }

        self.emit_plain_nonleaf_prologue();
        self.emit_call(
            name,
            arguments,
            Some(Eabi::general_result().number),
            false,
        )?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Marshal a call through a named function-pointer parameter/local/global
    /// and leave its address in r12 for an unlinked sibling transfer.
    fn emit_named_indirect_sibling_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        if let Some(pointer_register) = self.locations.get(name).map(|location| location.register) {
            self.stage_indirect_callee(pointer_register);
            self.emit_arguments(arguments, name)?;
        } else if self.globals.contains_key(name) {
            self.emit_arguments(arguments, name)?;
            self.emit_global_load_value(name, 12)?;
        } else {
            return Ok(false);
        }
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);
        Ok(true)
    }

    /// Lower a trivial terminal call as a sibling branch when the resolved
    /// direct- or indirect-call policy enables it.
    ///
    /// The arguments are still marshaled through the ordinary call path; only
    /// the link-producing call and now-dead epilogue are replaced. Restricting
    /// this to a body with no locals, guards, or preceding statements guarantees
    /// there is no live frame state to restore before transferring control.
    pub(crate) fn try_tail_call(&mut self, function: &Function) -> Compilation<bool> {
        if !function.locals.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || self.variadic_definition
        {
            return Ok(false);
        }

        if function.statements.is_empty() {
            let Some(Expression::Call { name, arguments }) = function.return_expression.as_ref()
            else {
                return Ok(false);
            };
            // The entire return value is the call result, so an indirect
            // sibling transfer preserves the callee's EABI result registers
            // without needing declaration-only return metadata here.
            if self.behavior.terminal_indirect_tail_call
                && self.emit_named_indirect_sibling_call(name, arguments)?
            {
                return Ok(true);
            }
            let allocator_tail_call = self.behavior.terminal_indirect_tail_call
                && allocator_pointer_return_is_compatible(name, function.return_type);
            if (!self.behavior.tail_call_optimization && !allocator_tail_call)
                || self.locations.contains_key(name)
                || self.globals.contains_key(name)
                || self.variadic_callees.contains(name)
                || (self.call_return_types.get(name) != Some(&function.return_type)
                    && !allocator_pointer_return_is_compatible(name, function.return_type))
            {
                return Ok(false);
            }

            self.emit_arguments(arguments, name)?;
            self.record_relocation(RelocationKind::Rel24, name);
            self.output.instructions.push(Instruction::BranchExternal {
                target: name.clone(),
            });
            return Ok(true);
        }

        // A terminal void statement call through a memory-resident function pointer is
        // the indirect sibling-call counterpart: load the callee, then transfer
        // with `bctr` without creating a frame or overwriting LR. This is a
        // default 4.x optimizer behavior, kept separate from the explicit IPA
        // policy used by direct sibling calls.
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        let [Statement::Expression(call)] = function.statements.as_slice() else {
            return Ok(false);
        };
        if let Expression::Call { name, arguments } = call {
            if self.behavior.terminal_indirect_tail_call
                && self.emit_named_indirect_sibling_call(name, arguments)?
            {
                return Ok(true);
            }
            if !self.behavior.tail_call_optimization
                || self.locations.contains_key(name)
                || self.globals.contains_key(name)
                || self.variadic_callees.contains(name)
                || self.call_return_types.get(name) != Some(&Type::Void)
            {
                return Ok(false);
            }
            self.emit_arguments(arguments, name)?;
            self.record_relocation(RelocationKind::Rel24, name);
            self.output.instructions.push(Instruction::BranchExternal {
                target: name.clone(),
            });
            return Ok(true);
        }
        if !self.behavior.terminal_indirect_tail_call {
            return Ok(false);
        }
        let Expression::CallThrough { target, arguments } = call else {
            return Ok(false);
        };
        if !arguments.is_empty()
            || !matches!(
                target.as_ref(),
                Expression::Dereference { .. } | Expression::Member { .. }
            )
        {
            return Ok(false);
        }

        self.evaluate(target, Type::UnsignedInt, 12)?;
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);
        Ok(true)
    }
}

/// The EABI allocators return `void*`, which is ABI-identical to every object
/// pointer return even though the source-level pointee type differs.
fn allocator_pointer_return_is_compatible(name: &str, return_type: Type) -> bool {
    crate::allocation_operator_returns_pointer(name)
        && matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. })
}
