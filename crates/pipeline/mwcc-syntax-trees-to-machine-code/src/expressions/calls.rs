//! Call emission and argument marshaling.

use super::call_argument_types::{classify_call_argument, CallArgumentPlacement};
#[allow(unused_imports)]
use super::*;
use mwcc_versions::FrameConvention;

impl Generator {
    /// Preserve an indirect callee across argument marshaling in r12. The copy
    /// instruction follows the generation's ordinary register-copy encoding.
    pub(crate) fn stage_indirect_callee(&mut self, register: u8) {
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => self.output.instructions.push(Instruction::Or {
                a: 12,
                s: register,
                b: register,
            }),
            FrameConvention::LinkageFirst => {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 12,
                    a: register,
                    immediate: 0,
                })
            }
        }
    }

    pub(crate) fn emit_indirect_branch_and_link(&mut self, register: u8) {
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.output
                    .instructions
                    .push(Instruction::MoveToCountRegister { s: register });
                self.output
                    .instructions
                    .push(Instruction::BranchToCountRegisterAndLink);
            }
            FrameConvention::LinkageFirst => {
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: register });
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegisterAndLink);
            }
        }
    }

    /// Emit a primary-vtable virtual call. The object is both the implicit
    /// first EABI argument and the address used for dispatch. Argument
    /// marshaling runs first; unsupported call-bearing later arguments defer in
    /// the ordinary argument scheduler before any unsafe sequence is emitted.
    pub(crate) fn emit_virtual_call(
        &mut self,
        object: &Expression,
        vptr_offset: u16,
        slot_offset: u16,
        variadic: bool,
        arguments: &[Expression],
        destination: Option<u8>,
        float_result: bool,
    ) -> Compilation<()> {
        self.emit_virtual_call_with_hidden_result(
            object,
            vptr_offset,
            slot_offset,
            variadic,
            arguments,
            None,
            destination,
            float_result,
        )
    }

    /// Emit a virtual call returning an aggregate through the EABI hidden
    /// result pointer. The hidden address occupies r3 and shifts the object
    /// (`this`) to r4; dispatch must therefore load through r4 rather than the
    /// scalar-return convention's r3.
    pub(crate) fn emit_virtual_call_with_aggregate_result(
        &mut self,
        object: &Expression,
        vptr_offset: u16,
        slot_offset: u16,
        variadic: bool,
        arguments: &[Expression],
        result_address: &Expression,
    ) -> Compilation<()> {
        self.emit_virtual_call_with_hidden_result(
            object,
            vptr_offset,
            slot_offset,
            variadic,
            arguments,
            Some(result_address),
            None,
            false,
        )
    }

    /// Lower an assignment from an aggregate-returning virtual call into the
    /// destination's frame slot. Both the ordinary and structured body owners
    /// must use the hidden-result EABI instead of treating the aggregate as a
    /// scalar register value.
    pub(crate) fn try_emit_frame_aggregate_virtual_assignment(
        &mut self,
        name: &str,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::VirtualCall {
            object,
            vptr_offset,
            slot_offset,
            return_type: Type::Struct { size, .. },
            variadic,
            arguments,
        } = value
        else {
            return Ok(false);
        };
        if !self.frame_slots.get(name).is_some_and(|slot| {
            matches!(slot.value_type, Type::Struct { size: slot_size, .. } if slot_size == *size)
                && !slot.is_array
        }) {
            return Ok(false);
        }
        let result_address = Expression::AddressOf {
            operand: Box::new(Expression::Variable(name.to_string())),
        };
        self.emit_virtual_call_with_aggregate_result(
            object,
            *vptr_offset,
            *slot_offset,
            *variadic,
            arguments,
            &result_address,
        )?;
        Ok(true)
    }

    fn emit_virtual_call_with_hidden_result(
        &mut self,
        object: &Expression,
        vptr_offset: u16,
        slot_offset: u16,
        variadic: bool,
        arguments: &[Expression],
        hidden_result: Option<&Expression>,
        destination: Option<u8>,
        float_result: bool,
    ) -> Compilation<()> {
        let object_argument = match object {
            Expression::Variable(name)
                if self.frame_slots.get(name).is_some_and(|slot| {
                    matches!(slot.value_type, Type::Struct { .. }) && !slot.is_array
                }) =>
            {
                Expression::AddressOf {
                    operand: Box::new(object.clone()),
                }
            }
            _ => object.clone(),
        };
        let object_register = Eabi::FIRST_GENERAL_ARGUMENT + u8::from(hidden_result.is_some());
        let dispatch_register = if let Some(result_address) = hidden_result {
            // With a saved leaf receiver and no explicit arguments, MWCC stages
            // `this` in r4 before forming the hidden-result address in r3. The
            // original saved home remains live and is used for vtable dispatch.
            // Keeping that provenance avoids an unnecessary dependence on r4.
            let saved_receiver = arguments
                .is_empty()
                .then(|| self.leaf_info(&object_argument).ok())
                .flatten()
                .filter(|(register, width, _)| *register >= 14 && *width == 32)
                .map(|(register, _, _)| register);
            if let Some(receiver) = saved_receiver {
                self.evaluate_general(&object_argument, object_register)?;
                self.evaluate_general(result_address, Eabi::FIRST_GENERAL_ARGUMENT)?;
                receiver
            } else {
                let mut all_arguments = Vec::with_capacity(arguments.len() + 2);
                all_arguments.push(result_address.clone());
                all_arguments.push(object_argument);
                all_arguments.extend_from_slice(arguments);
                self.emit_arguments(&all_arguments, "<virtual>")?;
                object_register
            }
        } else {
            let mut all_arguments = Vec::with_capacity(arguments.len() + 1);
            all_arguments.push(object_argument);
            all_arguments.extend_from_slice(arguments);
            self.emit_arguments(&all_arguments, "<virtual>")?;
            object_register
        };

        let vptr_offset = i16::try_from(vptr_offset)
            .map_err(|_| Diagnostic::error("a virtual-table pointer offset is out of range"))?;
        let slot_offset = i16::try_from(slot_offset)
            .map_err(|_| Diagnostic::error("a virtual-table slot offset is out of range"))?;
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: dispatch_register,
            offset: vptr_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 12,
            offset: slot_offset,
        });
        if variadic {
            self.output
                .instructions
                .push(Instruction::ConditionRegisterClear { d: 6 });
        }
        self.emit_indirect_branch_and_link(12);
        if let Some(destination) = destination {
            let result = if float_result {
                Eabi::float_result().number
            } else {
                Eabi::general_result().number
            };
            if destination != result {
                self.output.instructions.push(if float_result {
                    Instruction::FloatMove {
                        d: destination,
                        b: result,
                    }
                } else {
                    Instruction::move_register(destination, result)
                });
            }
        }
        Ok(())
    }

    /// Emit a direct call. Arguments are placed in the EABI argument registers,
    /// then `bl name`; the result (in r3 / f1) is moved to `destination` when one
    /// is wanted (a discarded call statement passes `None`).
    pub(crate) fn emit_call(
        &mut self,
        name: &str,
        arguments: &[Expression],
        destination: Option<u8>,
        float_result: bool,
    ) -> Compilation<()> {
        // An indirect call through a function-pointer variable (a parameter/local held in
        // a register): copy it to r12 before the arguments (which would overwrite its
        // register), then `mtctr r12; bctrl`. A named function is the direct `bl` below.
        if let Some(pointer_register) = self.locations.get(name).map(|location| location.register) {
            self.stage_indirect_callee(pointer_register);
            self.emit_arguments(arguments, name)?;
            self.emit_indirect_branch_and_link(12);
            if let Some(destination) = destination {
                let result = if float_result {
                    Eabi::float_result().number
                } else {
                    Eabi::general_result().number
                };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove {
                            d: destination,
                            b: result,
                        }
                    } else {
                        Instruction::move_register(destination, result)
                    });
                }
            }
            return Ok(());
        }
        // An indirect call through a GLOBAL function pointer: the pointer lives in
        // memory, so loading it into r12 doesn't clobber the argument registers — set up
        // the arguments, load the pointer, then `mtctr r12; bctrl`. (The saved-LR store
        // stays in the prologue here, since no `mr r12` setup precedes it.)
        if self.globals.contains_key(name) {
            self.emit_arguments(arguments, name)?;
            self.emit_global_load_value(name, 12)?;
            self.emit_indirect_branch_and_link(12);
            if let Some(destination) = destination {
                let result = if float_result {
                    Eabi::float_result().number
                } else {
                    Eabi::general_result().number
                };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove {
                            d: destination,
                            b: result,
                        }
                    } else {
                        Instruction::move_register(destination, result)
                    });
                }
            }
            return Ok(());
        }
        // A call through a DECLARED LOCAL that never got a register (a function-pointer
        // local no path allocated) must not fall through to the direct call below — that
        // would emit `bl <local>` with a relocation against the local's NAME (a link
        // error or a call to an unrelated symbol). Defer instead.
        if self.known_locals.contains(name) {
            return Err(Diagnostic::error("a call through an unallocated function-pointer local is not supported yet (roadmap)"));
        }
        // A call through a function-pointer PARAMETER would otherwise emit
        // `bl <param-name>` with a bogus relocation — defer.
        if self.locations.contains_key(name) {
            return Err(Diagnostic::error(
                "a call through a function-pointer parameter is not supported yet (roadmap)",
            ));
        }
        self.emit_arguments(arguments, name)?;
        if self.variadic_callees.contains(name) {
            self.output
                .instructions
                .push(Instruction::ConditionRegisterClear { d: 6 });
        }
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_string(),
        });
        if let Some(destination) = destination {
            let result = if float_result {
                Eabi::float_result().number
            } else {
                Eabi::general_result().number
            };
            if destination != result {
                self.output.instructions.push(if float_result {
                    Instruction::FloatMove {
                        d: destination,
                        b: result,
                    }
                } else {
                    Instruction::move_register(destination, result)
                });
            }
        }
        Ok(())
    }

    /// Place call arguments in the EABI argument registers (r3.. / f1..). Each is
    /// evaluated into its positional register; passthrough parameters are already
    /// in place, so this is a no-op for them.
    pub(crate) fn emit_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<()> {
        // A CALL in a non-first argument clobbers the argument registers already holding earlier
        // arguments (a call returns in r3 and clobbers r3–r12), and its own result lands in r3 rather
        // than the argument's positional register. mwcc evaluates such arguments RIGHT-first, preserving
        // the earlier results in callee-saved registers — a schedule not modeled here. Evaluating them
        // left-to-right would overwrite the earlier arguments (`s(5, f())`, `s(f(), g())`), so defer.
        // (A call in the FIRST argument alone is fine: later constant/in-place arguments do not clobber
        // its r3 result, e.g. `s(f(), 5)`.)
        // A float-returning SECOND argument beside a callee-saved first leaf is
        // simpler: evaluate the nested call into f1, then copy the preserved leaf
        // into r3. Measured in `outer(fp, float_call(fp->member))` as `bl; mr
        // r3,r31; bl`; no intermediate save or argument shuffle is needed.
        if let [first @ Expression::Variable(first_name), second @ Expression::Call {
            name: second_name, ..
        }] = arguments
        {
            let parameter_types = self.call_parameter_types.get(name);
            let first_parameter = parameter_types.and_then(|types| types.first()).copied();
            let second_parameter = parameter_types.and_then(|types| types.get(1)).copied();
            let first_is_general =
                first_parameter.is_some_and(|ty| !matches!(ty, Type::Float | Type::Double));
            let second_is_float = matches!(second_parameter, Some(Type::Float | Type::Double));
            let call_returns_float = matches!(
                self.call_return_types.get(second_name),
                Some(Type::Float | Type::Double)
            );
            let first_survives_call = self.globals.contains_key(first_name.as_str())
                || self
                    .locations
                    .get(first_name.as_str())
                    .is_some_and(|location| location.register >= 14);
            if first_is_general && second_is_float && call_returns_float && first_survives_call {
                self.evaluate_float(second, Eabi::FIRST_FLOAT_ARGUMENT)?;
                self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
                return Ok(());
            }
        }
        // A general-class SECOND argument may likewise contain a nested call
        // when the first leaf already survives calls in a callee-saved register
        // (or is reloadable from a global). Evaluate the nested value into r4,
        // then restore the first argument into r3. This is the C++
        // `registerState(this, new State)` shape: allocation and construction
        // finish before the preserved object pointer is marshaled.
        if let [first @ Expression::Variable(first_name), second] = arguments {
            let parameter_types = self.call_parameter_types.get(name);
            let both_general = parameter_types.is_some_and(|types| {
                types.len() >= 2
                    && !matches!(types[0], Type::Float | Type::Double)
                    && !matches!(types[1], Type::Float | Type::Double)
            });
            let first_survives_call = self.globals.contains_key(first_name.as_str())
                || self
                    .locations
                    .get(first_name.as_str())
                    .is_some_and(|location| location.register >= 14);
            if both_general && first_survives_call && expression_has_call(second) {
                self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
                self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
                return Ok(());
            }
        }
        // Two floating arguments use an independent FPR sequence. When the
        // first value already has a callee-saved home, a call-bearing second
        // argument can be evaluated right-first into f2 and the first restored
        // into f1 afterward. This is the nested `angDist(saved, roundAng(...))`
        // schedule; no caller-saved value remains live across the nested call.
        if let [first @ Expression::Variable(first_name), second] = arguments {
            let both_float = self.call_parameter_types.get(name).is_some_and(|types| {
                types.len() >= 2
                    && matches!(types[0], Type::Float | Type::Double)
                    && matches!(types[1], Type::Float | Type::Double)
            });
            let first_survives_call = self
                .locations
                .get(first_name.as_str())
                .is_some_and(|location| {
                    location.class == ValueClass::Float && location.register >= 14
                });
            if both_float
                && first_survives_call
                && self.is_float_value(second)
                && expression_has_call(second)
            {
                self.evaluate_float(second, Eabi::FIRST_FLOAT_ARGUMENT + 1)
                    .map_err(|mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (while scheduling call-bearing second float argument to '{name}')"
                        ));
                        diagnostic
                    })?;
                self.evaluate_float(first, Eabi::FIRST_FLOAT_ARGUMENT)
                    .map_err(|mut diagnostic| {
                        diagnostic.message.push_str(&format!(
                            " (while restoring first float argument to '{name}')"
                        ));
                        diagnostic
                    })?;
                return Ok(());
            }
        }
        // `h(gg, g())` / `h(arr, g())` — a GLOBAL first argument and an argument-free call
        // as the SECOND. The global is reloadable (it lives in memory), so mwcc needs no
        // callee-saved register: it evaluates the call FIRST (its result in r3), then
        // materializes the first argument around the copy. Three measured forms:
        //   scalar:      bl g; mr r4,r3; lwz r3,gg
        //   small array: bl g; mr r4,r3; li r3,arr@sda21          (SDA21, size <= 8)
        //   large array: bl g; lis r5,arr@ha; mr r4,r3; addi r3,r5,arr@l
        // — the large array's `lis` fills the call-return latency slot BETWEEN the bl and
        // the mr, through r5 (the first register past both arguments). This is the first
        // slice of the callee-saved argument scheduler (the __register_fragment(
        // _eti_init_info, GetR2()) shape); the param-first form (which must save the param
        // across the call in a callee-saved register) still defers below.
        if let [Expression::Variable(global), second @ Expression::Call {
            arguments: call_arguments,
            ..
        }] = arguments
        {
            if self.globals.contains_key(global.as_str()) && call_arguments.is_empty() {
                let first_register = Eabi::FIRST_GENERAL_ARGUMENT;
                if let Some(&total_size) = self.global_array_sizes.get(global.as_str()) {
                    let small = self.behavior.global_addressing == GlobalAddressing::SmallData
                        && total_size <= 8;
                    let global = global.clone();
                    self.evaluate_general(second, first_register)?; // bl g -> r3
                    if small {
                        self.emit_integer_materialization_copy(first_register + 1, first_register); // pointer result -> argument r4
                        self.record_relocation(RelocationKind::EmbSda21, &global);
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: first_register,
                            a: 0,
                            immediate: 0,
                        }); // li r3,arr@sda21
                    } else {
                        let high = first_register + 2; // r5 — past both argument registers
                        self.emit_address_high(high, &global); // lis r5,arr@ha
                        self.emit_integer_materialization_copy(first_register + 1, first_register); // pointer result -> argument r4
                        self.record_relocation(RelocationKind::Addr16Lo, &global);
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: first_register,
                            a: high,
                            immediate: 0,
                        }); // addi r3,r5,arr@l
                    }
                    return Ok(());
                }
                self.evaluate_general(second, first_register)?; // bl g -> r3
                self.output.instructions.push(Instruction::move_register(
                    first_register + 1,
                    first_register,
                )); // mr r4,r3
                self.evaluate_general(&arguments[0], first_register)?; // lwz r3,gg
                return Ok(());
            }
        }
        if arguments.iter().skip(1).any(expression_has_call) {
            let index = arguments
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(index, argument)| expression_has_call(argument).then_some(index))
                .expect("the guard found a call-bearing argument");
            return Err(Diagnostic::error(format!(
                "a call in argument {index} to '{name}' needs the callee-saved argument scheduler (roadmap)"
            )));
        }
        // The SAME global read in two argument positions loads once in mwcc, which copies it to the
        // second register (`lwz r3,g; mr r4,r3`); our per-argument evaluation loads it in each — wrong
        // bytes. Defer a global variable that appears as two arguments. (A register-resident parameter
        // passed twice is a free re-read and stays byte-exact; two DIFFERENT globals load independently.)
        for (index, argument) in arguments.iter().enumerate() {
            if let Expression::Variable(name) = argument {
                if self.globals.contains_key(name.as_str())
                    && arguments[index + 1..].iter().any(|other| matches!(other, Expression::Variable(other_name) if other_name == name))
                {
                    return Err(Diagnostic::error("the same global read in two arguments needs load-once reuse (roadmap)"));
                }
            }
        }
        let direct_call = !self.globals.contains_key(name) && !self.locations.contains_key(name);
        if self.try_emit_reverse_indexed_address_tail_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        if self.try_emit_member_and_located_float_product_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        if self.try_emit_large_string_line_arguments(arguments, direct_call)? {
            return Ok(());
        }
        if self.try_emit_interleaved_member_float_forward_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        if self.try_emit_member_prefixed_float_shift_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        if self.try_emit_member_float_abs_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        // With packed `@stringBaseN` literals, MWCC schedules the complete
        // absolute address of a second string argument before a cheap first
        // member-address adjustment. This keeps the dependent `addi` pair
        // adjacent and leaves the independent object adjustment immediately
        // before the call (`f(&object.member, "name")`).
        let first_is_member_address = arguments.first().is_some_and(|argument| {
            matches!(argument, Expression::MemberAddress { .. })
                || matches!(
                    argument,
                    Expression::AddressOf { operand }
                        if matches!(operand.as_ref(), Expression::Member { .. })
                )
        });
        if self.behavior.string_literals_packed
            && first_is_member_address
            && matches!(arguments.get(1), Some(Expression::StringLiteral(_)))
            && arguments.len() == 2
        {
            self.evaluate_general(&arguments[1], Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
            self.evaluate_general(&arguments[0], Eabi::FIRST_GENERAL_ARGUMENT)?;
            return Ok(());
        }
        if self.try_emit_global_member_and_endangered_member_address(arguments, direct_call)? {
            return Ok(());
        }
        if self.try_emit_shared_base_bitfield_arguments(arguments, direct_call)? {
            return Ok(());
        }
        if self.try_emit_same_base_member_forward_arguments(arguments, direct_call)? {
            return Ok(());
        }
        if self.try_emit_member_constant_arguments(arguments, name, direct_call)? {
            return Ok(());
        }
        // A CONSTANT argument that follows a GLOBAL-LOAD argument: mwcc materializes
        // the constants GREEDY-EARLY — their `li`s emit ahead of the load, and the
        // save scheduler then hoists them into the prologue's mflr latency slot
        // (measured `h(gi, 5)`: stwu; mflr; li r4,5; stw r0; lwz r3,@gi; bl).
        // Lifted for a DIRECT call with ONE word-global load plus i16 constants
        // (up to three arguments); other mixes keep the defer.
        {
            let mut seen_global_load = false;
            let mut constant_after_global = false;
            for argument in arguments {
                match argument {
                    Expression::Variable(name) if self.globals.contains_key(name.as_str()) => {
                        seen_global_load = true
                    }
                    Expression::IntegerLiteral(_) if seen_global_load => {
                        constant_after_global = true
                    }
                    _ => {}
                }
            }
            let address_array_constant = match arguments {
                [Expression::AddressOf { operand }, Expression::Variable(array), Expression::IntegerLiteral(value)]
                    if (i16::MIN as i64..=i16::MAX as i64).contains(value) =>
                {
                    let Expression::Variable(addressed) = operand.as_ref() else {
                        return Err(Diagnostic::error(
                            "a constant argument after a global load needs the LR-store-latency schedule (roadmap)",
                        ));
                    };
                    let addressed_size = self.globals.get(addressed.as_str()).map(|ty| match ty {
                        Type::Struct { size, .. } => u32::from(*size),
                        other => u32::from(other.width()).div_ceil(8),
                    });
                    let array_size = self.global_array_sizes.get(array.as_str()).copied();
                    let absolute = self.behavior.global_addressing == GlobalAddressing::Absolute;
                    match (addressed_size, array_size) {
                        (Some(first), Some(second)) if absolute || (first > 8 && second > 8) => {
                            Some((addressed.clone(), array.clone(), *value as i16))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some((addressed, array, value)) = address_array_constant {
                // Two large object addresses plus an i16 constant use both
                // address-high instructions first, then fill each dependent
                // addi's latency slot: lis r3; lis r4; addi r3; li r5; addi r4.
                self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT, &addressed);
                self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT + 1, &array);
                self.record_relocation(RelocationKind::Addr16Lo, &addressed);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: Eabi::FIRST_GENERAL_ARGUMENT,
                    a: Eabi::FIRST_GENERAL_ARGUMENT,
                    immediate: 0,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: Eabi::FIRST_GENERAL_ARGUMENT + 2,
                    a: 0,
                    immediate: value,
                });
                self.record_relocation(RelocationKind::Addr16Lo, &array);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    a: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    immediate: 0,
                });
                return Ok(());
            }
            if constant_after_global {
                if self.try_emit_unscheduled_global_constant_arguments(arguments, direct_call)? {
                    return Ok(());
                }
                if self.try_emit_absolute_short_global_constant_arguments(arguments, direct_call)? {
                    return Ok(());
                }
                let mut global_argument: Option<(usize, String)> = None;
                let mut constants: Vec<(usize, i16)> = Vec::new();
                let mut simple = direct_call && arguments.len() <= 3;
                for (position, argument) in arguments.iter().enumerate() {
                    if !simple {
                        break;
                    }
                    match argument {
                        Expression::Variable(variable)
                            if self.globals.contains_key(variable.as_str()) =>
                        {
                            if global_argument.is_some()
                                || !matches!(
                                    self.globals.get(variable.as_str()),
                                    Some(Type::Int | Type::UnsignedInt)
                                )
                            {
                                simple = false;
                            } else {
                                global_argument = Some((position, variable.clone()));
                            }
                        }
                        Expression::IntegerLiteral(value)
                            if (i16::MIN as i64..=i16::MAX as i64).contains(value) =>
                        {
                            constants.push((position, *value as i16));
                        }
                        _ => simple = false,
                    }
                }
                match (simple, global_argument) {
                    (true, Some((global_position, global_name)))
                        if constants.len() + 1 == arguments.len() =>
                    {
                        for &(position, value) in &constants {
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: Eabi::FIRST_GENERAL_ARGUMENT + position as u8,
                                a: 0,
                                immediate: value,
                            });
                        }
                        self.emit_global_load(
                            &global_name,
                            Eabi::FIRST_GENERAL_ARGUMENT + global_position as u8,
                        )?;
                        return Ok(());
                    }
                    _ => {
                        return Err(Diagnostic::error(
                            "a constant argument after a global load needs the LR-store-latency schedule (roadmap)",
                        ));
                    }
                }
            }
        }
        // A `&global + n` argument materializes as `li rD,0; addi rD,rD,k`. Alongside
        // other arguments mwcc reorders the leading `li`s (the offset arg's base first)
        // in a way not yet modeled, so defer rather than mis-schedule. A lone such
        // argument is fine (the single-`li` hoist matches).
        if arguments.len() >= 2
            && arguments
                .iter()
                .any(|argument| self.is_global_address_arithmetic(argument))
        {
            return Err(Diagnostic::error(
                "a `&global + n` argument alongside others needs the multi-arg schedule (roadmap)",
            ));
        }
        // Two word loads from ONE pointer base, where loading the first clobbers the base
        // register (`g(p->a, p->b)` / `g(p[0], p[1])` with `p` in r3): mwcc pre-copies the base to
        // the second argument register, then loads each — `mr r4,r3; lwz r3,off0(r3); lwz
        // r4,off1(r4)` (without the pre-copy, ours would load p[1] through p[0], a MISCOMPILE). The
        // pre-copy `mr` is hoisted into the non-leaf prologue slot by the body emitter. Word members
        // and constant-index word subscripts of the base qualify; the general N-argument / mixed-width
        // choreography is the allocator's.
        if let [argument0, argument1] = arguments {
            let base_register = Eabi::FIRST_GENERAL_ARGUMENT;
            let copy_register = base_register + 1;
            // (base pointer name, byte offset, load pointee) for a word `p->m` / `p[k]` argument.
            let word_pointer_load =
                |generator: &Self, argument: &Expression| -> Option<(String, i16, Pointee)> {
                    match argument {
                        Expression::Member {
                            base,
                            offset,
                            member_type,
                            index_stride: None,
                        } => {
                            let Expression::Variable(name) = base.as_ref() else {
                                return None;
                            };
                            let is_word = matches!(
                                member_type,
                                Type::Int
                                    | Type::UnsignedInt
                                    | Type::Pointer(_)
                                    | Type::StructPointer { .. }
                            );
                            if !is_word {
                                return None;
                            }
                            Some((
                                name.clone(),
                                i16::try_from(*offset as i64).ok()?,
                                pointee_of_type(*member_type)?,
                            ))
                        }
                        Expression::Index { base, index } => {
                            let Expression::Variable(name) = base.as_ref() else {
                                return None;
                            };
                            let constant = constant_value(index)?;
                            let pointee = generator.locations.get(name.as_str())?.pointee?;
                            if pointee.size() != 4 {
                                return None; // a word (int/pointer) element only
                            }
                            Some((
                                name.clone(),
                                i16::try_from(constant * pointee.size() as i64).ok()?,
                                pointee,
                            ))
                        }
                        _ => None,
                    }
                };
            if let (Some((pointer0, offset0, pointee0)), Some((pointer1, offset1, pointee1))) = (
                word_pointer_load(self, argument0),
                word_pointer_load(self, argument1),
            ) {
                // Only two DIFFERENT loads (`g(p[0], p[1])`) take the base-preservation. The SAME
                // load twice (`g(p[0], p[0])`) is a load-once-copy in mwcc (`lwz r3,off(r3); mr
                // r4,r3`) whose .text we match but whose @N anonymous-symbol numbering diverges (the
                // low-impact object-writer seam), so leave it to the argument-clobber defer below.
                if pointer0 == pointer1
                    && !(offset0 == offset1 && pointee0 == pointee1)
                    && self
                        .locations
                        .get(pointer0.as_str())
                        .map(|location| location.register)
                        == Some(base_register)
                {
                    self.output
                        .instructions
                        .push(Instruction::move_register(copy_register, base_register));
                    self.output.instructions.push(displacement_load(
                        pointee0,
                        base_register,
                        base_register,
                        offset0,
                    )?);
                    self.output.instructions.push(displacement_load(
                        pointee1,
                        copy_register,
                        copy_register,
                        offset1,
                    )?);
                    return Ok(());
                }
            }
        }
        // THREE or FOUR word loads from ONE pointer base (`g(p->a, p->b, p->c[, p->d])`):
        // mwcc pre-copies the base to the LAST argument register, loads the first
        // argument through the dying original base, and the rest through the copy —
        // the copy's own load comes last. The load ORDER is measured per count:
        // three arguments hoist arg1's load ahead (`mr r5,r3; lwz r4,4(r5);
        // lwz r3,0(r3); lwz r5,8(r5)`), four go in argument order (`mr r6,r3;
        // lwz r3,0(r3); lwz r4,4(r6); lwz r5,8(r6); lwz r6,12(r6)`). Distinct
        // offsets only (a repeated member diverges on the @N seam, as in the
        // two-argument case).
        if matches!(arguments.len(), 3 | 4) {
            let base_register = Eabi::FIRST_GENERAL_ARGUMENT;
            let member_load = |argument: &Expression| -> Option<(String, i16, Pointee)> {
                match argument {
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    } => {
                        let Expression::Variable(name) = base.as_ref() else {
                            return None;
                        };
                        let is_word = matches!(
                            member_type,
                            Type::Int
                                | Type::UnsignedInt
                                | Type::Pointer(_)
                                | Type::StructPointer { .. }
                        );
                        if !is_word {
                            return None;
                        }
                        Some((
                            name.clone(),
                            i16::try_from(*offset as i64).ok()?,
                            pointee_of_type(*member_type)?,
                        ))
                    }
                    _ => None,
                }
            };
            let loads: Vec<Option<(String, i16, Pointee)>> =
                arguments.iter().map(member_load).collect();
            if loads.iter().all(Option::is_some) {
                let loads: Vec<(String, i16, Pointee)> =
                    loads.into_iter().map(Option::unwrap).collect();
                let one_base = loads.iter().all(|(name, _, _)| name == &loads[0].0);
                let distinct = loads
                    .iter()
                    .map(|(_, offset, _)| offset)
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    == loads.len();
                if one_base
                    && distinct
                    && self
                        .locations
                        .get(loads[0].0.as_str())
                        .map(|location| location.register)
                        == Some(base_register)
                {
                    let copy_register = base_register + arguments.len() as u8 - 1;
                    self.output
                        .instructions
                        .push(Instruction::move_register(copy_register, base_register));
                    let order: &[usize] = if arguments.len() == 3 {
                        &[1, 0, 2]
                    } else {
                        &[0, 1, 2, 3]
                    };
                    for &slot in order {
                        let (_, offset, pointee) = loads[slot];
                        let source = if slot == 0 {
                            base_register
                        } else {
                            copy_register
                        };
                        self.output.instructions.push(displacement_load(
                            pointee,
                            base_register + slot as u8,
                            source,
                            offset,
                        )?);
                    }
                    return Ok(());
                }
            }
        }
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        let mut folded_float_arguments: Vec<(u64, bool, u8)> = Vec::new();
        // Preserve a later word argument whose current register is the target of
        // an earlier argument remap. Build 163 copies that endangered value to
        // its final ABI slot first, then fills the earlier slots. Restrict this
        // to the all-general, one-word prefix where argument index maps directly
        // to r3+rN; mixed floating/wide calls retain their dedicated schedulers.
        let prematerialized_general = (1..arguments.len()).find_map(|later| {
            let source = self.leaf_info(&arguments[later]).ok()?.0;
            let target = Eabi::FIRST_GENERAL_ARGUMENT.checked_add(later as u8)?;
            if source == target
                || !(0..later).any(|earlier| {
                    Eabi::FIRST_GENERAL_ARGUMENT + earlier as u8 == source
                        && self
                            .leaf_info(&arguments[earlier])
                            .map(|(register, width, _)| register != source && width == 32)
                            .unwrap_or(false)
                })
                || !arguments[..=later]
                    .iter()
                    .enumerate()
                    .all(|(index, argument)| {
                        !self.is_float_value(argument)
                            && !matches!(
                                self.call_parameter_types
                                    .get(name)
                                    .and_then(|types| types.get(index)),
                                Some(
                                    Type::Float
                                        | Type::Double
                                        | Type::LongLong
                                        | Type::UnsignedLongLong
                                )
                            )
                    })
            {
                return None;
            }
            Some((later, target, source))
        });
        if let Some((_, target, source)) = prematerialized_general {
            self.emit_integer_materialization_copy(target, source);
        }
        for (index, argument) in arguments.iter().enumerate() {
            let parameter_type = self
                .call_parameter_types
                .get(name)
                .and_then(|types| types.get(index))
                .copied();
            let placement = classify_call_argument(
                parameter_type,
                self.is_float_value(argument),
                constant_value(argument),
            )
            .map_err(|mut diagnostic| {
                diagnostic.message.push_str(&format!(
                    " (argument {index} to '{name}', parameter {parameter_type:?})"
                ));
                diagnostic
            })?;
            if let CallArgumentPlacement::Floating {
                parameter_type,
                folded_integer,
            } = placement
            {
                let constant = folded_integer.or_else(|| match argument {
                    Expression::FloatLiteral(value) => Some(*value),
                    _ => None,
                });
                if let Some(value) = constant {
                    let double = parameter_type == Type::Double;
                    let bits = if double {
                        value.to_bits()
                    } else {
                        u64::from((value as f32).to_bits())
                    };
                    if let Some((_, _, source)) =
                        folded_float_arguments
                            .iter()
                            .find(|(seen_bits, seen_double, _)| {
                                *seen_bits == bits && *seen_double == double
                            })
                    {
                        self.output.instructions.push(Instruction::FloatMove {
                            d: next_float,
                            b: *source,
                        });
                    } else {
                        self.load_float_literal(next_float, value, double);
                        folded_float_arguments.push((bits, double, next_float));
                    }
                } else {
                    self.evaluate(argument, parameter_type, next_float)
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (while evaluating floating argument {index} to '{name}')"
                            ));
                            diagnostic
                        })?;
                }
                next_float += 1;
            } else {
                if prematerialized_general.is_some_and(|(later, _, _)| later == index) {
                    next_general += 1;
                    continue;
                }
                // A narrow (char/short) argument to a parameter that is NOT wider is passed
                // WITHOUT the int promotion — `void g(char); g(char_a)` is just `bl g`, no
                // `extsb` (only a wider parameter, e.g. `void g(int)`, widens the argument).
                // Handled for the in-place case (the value already sits in the argument
                // register); a move or a non-leaf falls through to the widening eval.
                if let Some(parameter_type) = self
                    .call_parameter_types
                    .get(name)
                    .and_then(|types| types.get(index))
                {
                    if let Ok((register, width, _)) = self.leaf_info(argument) {
                        if width < 32
                            && (parameter_type.width() as u32) <= width as u32
                            && register == next_general
                        {
                            next_general += 1;
                            continue;
                        }
                    }
                }
                // An argument WIDER than a narrow (char/short) parameter must be narrowed to
                // the parameter type — `void g(char); g(int_a)` is `extsb r3,r3; bl g` (the C
                // conversion to `(char)`). That narrowing is not modeled, and mwcc schedules
                // the `extsb` into the non-leaf prologue (keystone), so defer rather than pass
                // the wide value un-narrowed: `g(256)` to a `char` parameter must pass 0, not
                // 256 (a miscompile). A constant is materialized in range; a narrow leaf /
                // load / global already fits and is handled by the passthrough above.
                if let Some(parameter_type) = self
                    .call_parameter_types
                    .get(name)
                    .and_then(|types| types.get(index))
                {
                    if (parameter_type.width() as u32) < 32 && constant_value(argument).is_none() {
                        let argument_is_narrow = match argument {
                            Expression::Variable(variable)
                                if self.locations.contains_key(variable.as_str()) =>
                            {
                                self.leaf_info(argument)
                                    .map(|(_, width, _)| width < 32)
                                    .unwrap_or(false)
                            }
                            Expression::Variable(variable) => self
                                .globals
                                .get(variable.as_str())
                                .map(|global| global.width() < 32)
                                .unwrap_or(false),
                            Expression::Dereference { pointer } => self
                                .dereferenced_width(pointer)
                                .is_some_and(|width| width < 32),
                            Expression::Index { base, .. } => self
                                .dereferenced_width(base)
                                .is_some_and(|width| width < 32),
                            Expression::Member { member_type, .. } => member_type.width() < 32,
                            _ => false,
                        };
                        if !argument_is_narrow {
                            // An IN-PLACE register leaf narrows with one op (extsb/extsh
                            // for a signed parameter, clrlwi 24/16 for unsigned) that the
                            // prologue hoist then schedules into the mflr->LR-store slot
                            // (measured: `void g(short); g(x)` -> extsh r3,r3 mid-prologue).
                            // A value NOT already in its argument register still defers.
                            if let Ok((register, _, _)) = self.leaf_info(argument) {
                                if register == next_general {
                                    let narrow = match parameter_type {
                                        Type::Char => Instruction::ExtendSignByte { a: register, s: register },
                                        Type::Short => Instruction::ExtendSignHalfword { a: register, s: register },
                                        Type::UnsignedChar => Instruction::ClearLeftImmediate { a: register, s: register, clear: 24 },
                                        Type::UnsignedShort => Instruction::ClearLeftImmediate { a: register, s: register, clear: 16 },
                                        _ => return Err(Diagnostic::error("an argument wider than a narrow parameter needs a narrowing conversion (roadmap)")),
                                    };
                                    self.output.instructions.push(narrow);
                                    next_general += 1;
                                    continue;
                                }
                            }
                            return Err(Diagnostic::error("an argument wider than a narrow parameter needs a narrowing conversion (roadmap)"));
                        }
                    }
                }
                // Honest guard: evaluating into this argument register must not
                // clobber a register a later argument still needs. mwcc handles
                // that (e.g. two members of one struct) by pre-copying the shared
                // base; that choreography is not modeled yet.
                //
                // A leaf argument already in its target register is a
                // passthrough regardless of arity: evaluating it emits no
                // instruction, so it cannot clobber a shared base needed by a
                // later member argument. Later actual moves/computations still
                // pass through this guard independently.
                let passthrough_in_place = self
                    .leaf_info(argument)
                    .map(|(register, _, _)| register == next_general)
                    .unwrap_or(false);
                if !passthrough_in_place
                    && arguments[index + 1..]
                        .iter()
                        .enumerate()
                        .any(|(offset, later)| {
                            let later_index = index + 1 + offset;
                            !prematerialized_general.is_some_and(|(prematerialized, _, _)| {
                                prematerialized == later_index
                            }) && self.registers_used_by(later).contains(&next_general)
                        })
                {
                    return Err(Diagnostic::error(
                        "argument would clobber a register a later argument needs (roadmap)",
                    ));
                }
                // In a MULTI-argument remap, a dying incoming parameter shifted
                // DOWN into an earlier ABI argument register is a value
                // materialization. Build 163 uses `addi d,s,0` for that direction;
                // a single-argument move and a duplicated earlier argument
                // (`g(a,a)`, r3 -> r4) remain `mr`. Mainline uses `mr` throughout.
                let downward_word_copy =
                    self.leaf_info(argument).ok().filter(|(source, width, _)| {
                        arguments.len() > 1
                            && *width == 32
                            && *source > next_general
                            && *source <= Eabi::LAST_GENERAL_ARGUMENT
                    });
                if let Some((source, _, _)) = downward_word_copy {
                    self.emit_integer_materialization_copy(next_general, source);
                } else {
                    self.evaluate_general(argument, next_general)?;
                }
                next_general += 1;
            }
        }
        Ok(())
    }
}
