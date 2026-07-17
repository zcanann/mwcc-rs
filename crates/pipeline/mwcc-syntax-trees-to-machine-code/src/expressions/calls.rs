//! Call emission and argument marshaling.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit a direct call. Arguments are placed in the EABI argument registers,
    /// then `bl name`; the result (in r3 / f1) is moved to `destination` when one
    /// is wanted (a discarded call statement passes `None`).
    pub(crate) fn emit_call(&mut self, name: &str, arguments: &[Expression], destination: Option<u8>, float_result: bool) -> Compilation<()> {
        // An indirect call through a function-pointer variable (a parameter/local held in
        // a register): copy it to r12 before the arguments (which would overwrite its
        // register), then `mtctr r12; bctrl`. A named function is the direct `bl` below.
        if let Some(pointer_register) = self.locations.get(name).map(|location| location.register) {
            self.output.instructions.push(Instruction::Or { a: 12, s: pointer_register, b: pointer_register });
            self.emit_arguments(arguments, name)?;
            self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
            self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
            if let Some(destination) = destination {
                let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove { d: destination, b: result }
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
            self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
            self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
            if let Some(destination) = destination {
                let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
                if destination != result {
                    self.output.instructions.push(if float_result {
                        Instruction::FloatMove { d: destination, b: result }
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
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink { target: name.to_string() });
        if let Some(destination) = destination {
            let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
            if destination != result {
                self.output.instructions.push(if float_result {
                    Instruction::FloatMove { d: destination, b: result }
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
    pub(crate) fn emit_arguments(&mut self, arguments: &[Expression], name: &str) -> Compilation<()> {
        // A CALL in a non-first argument clobbers the argument registers already holding earlier
        // arguments (a call returns in r3 and clobbers r3–r12), and its own result lands in r3 rather
        // than the argument's positional register. mwcc evaluates such arguments RIGHT-first, preserving
        // the earlier results in callee-saved registers — a schedule not modeled here. Evaluating them
        // left-to-right would overwrite the earlier arguments (`s(5, f())`, `s(f(), g())`), so defer.
        // (A call in the FIRST argument alone is fine: later constant/in-place arguments do not clobber
        // its r3 result, e.g. `s(f(), 5)`.)
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
        if let [Expression::Variable(global), second @ Expression::Call { arguments: call_arguments, .. }] = arguments {
            if self.globals.contains_key(global.as_str()) && call_arguments.is_empty() {
                let first_register = Eabi::FIRST_GENERAL_ARGUMENT;
                if let Some(&total_size) = self.global_array_sizes.get(global.as_str()) {
                    let small = self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
                    let global = global.clone();
                    self.evaluate_general(second, first_register)?; // bl g -> r3
                    if small {
                        self.output.instructions.push(Instruction::move_register(first_register + 1, first_register)); // mr r4,r3
                        self.record_relocation(RelocationKind::EmbSda21, &global);
                        self.output.instructions.push(Instruction::AddImmediate { d: first_register, a: 0, immediate: 0 }); // li r3,arr@sda21
                    } else {
                        let high = first_register + 2; // r5 — past both argument registers
                        self.emit_address_high(high, &global); // lis r5,arr@ha
                        self.output.instructions.push(Instruction::move_register(first_register + 1, first_register)); // mr r4,r3
                        self.record_relocation(RelocationKind::Addr16Lo, &global);
                        self.output.instructions.push(Instruction::AddImmediate { d: first_register, a: high, immediate: 0 }); // addi r3,r5,arr@l
                    }
                    return Ok(());
                }
                self.evaluate_general(second, first_register)?; // bl g -> r3
                self.output.instructions.push(Instruction::move_register(first_register + 1, first_register)); // mr r4,r3
                self.evaluate_general(&arguments[0], first_register)?; // lwz r3,gg
                return Ok(());
            }
        }
        if arguments.iter().skip(1).any(expression_has_call) {
            return Err(Diagnostic::error("a call in a non-first argument needs the callee-saved argument scheduler (roadmap)"));
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
        // A CONSTANT argument that follows a GLOBAL-LOAD argument: mwcc hoists the constant's `li` into
        // the mflr->LR-store latency slot of the non-leaf prologue (ahead of the global load), a
        // schedule our left-to-right emission (load, then `li`) does not reproduce. Defer. (A constant
        // BEFORE the global load — `s(5, gi)` — is already early and stays byte-exact.)
        {
            let mut seen_global_load = false;
            for argument in arguments {
                match argument {
                    Expression::Variable(name) if self.globals.contains_key(name.as_str()) => seen_global_load = true,
                    Expression::IntegerLiteral(_) if seen_global_load => {
                        return Err(Diagnostic::error("a constant argument after a global load needs the LR-store-latency schedule (roadmap)"));
                    }
                    _ => {}
                }
            }
        }
        // A `&global + n` argument materializes as `li rD,0; addi rD,rD,k`. Alongside
        // other arguments mwcc reorders the leading `li`s (the offset arg's base first)
        // in a way not yet modeled, so defer rather than mis-schedule. A lone such
        // argument is fine (the single-`li` hoist matches).
        if arguments.len() >= 2 && arguments.iter().any(|argument| self.is_global_address_arithmetic(argument)) {
            return Err(Diagnostic::error("a `&global + n` argument alongside others needs the multi-arg schedule (roadmap)"));
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
            let word_pointer_load = |generator: &Self, argument: &Expression| -> Option<(String, i16, Pointee)> {
                match argument {
                    Expression::Member { base, offset, member_type, index_stride: None } => {
                        let Expression::Variable(name) = base.as_ref() else { return None };
                        let is_word = matches!(member_type, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. });
                        if !is_word {
                            return None;
                        }
                        Some((name.clone(), i16::try_from(*offset as i64).ok()?, pointee_of_type(*member_type)?))
                    }
                    Expression::Index { base, index } => {
                        let Expression::Variable(name) = base.as_ref() else { return None };
                        let constant = constant_value(index)?;
                        let pointee = generator.locations.get(name.as_str())?.pointee?;
                        if pointee.size() != 4 {
                            return None; // a word (int/pointer) element only
                        }
                        Some((name.clone(), i16::try_from(constant * pointee.size() as i64).ok()?, pointee))
                    }
                    _ => None,
                }
            };
            if let (Some((pointer0, offset0, pointee0)), Some((pointer1, offset1, pointee1))) =
                (word_pointer_load(self, argument0), word_pointer_load(self, argument1))
            {
                // Only two DIFFERENT loads (`g(p[0], p[1])`) take the base-preservation. The SAME
                // load twice (`g(p[0], p[0])`) is a load-once-copy in mwcc (`lwz r3,off(r3); mr
                // r4,r3`) whose .text we match but whose @N anonymous-symbol numbering diverges (the
                // low-impact object-writer seam), so leave it to the argument-clobber defer below.
                if pointer0 == pointer1
                    && !(offset0 == offset1 && pointee0 == pointee1)
                    && self.locations.get(pointer0.as_str()).map(|location| location.register) == Some(base_register)
                {
                    self.output.instructions.push(Instruction::move_register(copy_register, base_register));
                    self.output.instructions.push(displacement_load(pointee0, base_register, base_register, offset0)?);
                    self.output.instructions.push(displacement_load(pointee1, copy_register, copy_register, offset1)?);
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
                    Expression::Member { base, offset, member_type, index_stride: None } => {
                        let Expression::Variable(name) = base.as_ref() else { return None };
                        let is_word = matches!(member_type, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. });
                        if !is_word {
                            return None;
                        }
                        Some((name.clone(), i16::try_from(*offset as i64).ok()?, pointee_of_type(*member_type)?))
                    }
                    _ => None,
                }
            };
            let loads: Vec<Option<(String, i16, Pointee)>> = arguments.iter().map(member_load).collect();
            if loads.iter().all(Option::is_some) {
                let loads: Vec<(String, i16, Pointee)> = loads.into_iter().map(Option::unwrap).collect();
                let one_base = loads.iter().all(|(name, _, _)| name == &loads[0].0);
                let distinct = loads.iter().map(|(_, offset, _)| offset).collect::<std::collections::HashSet<_>>().len() == loads.len();
                if one_base
                    && distinct
                    && self.locations.get(loads[0].0.as_str()).map(|location| location.register) == Some(base_register)
                {
                    let copy_register = base_register + arguments.len() as u8 - 1;
                    self.output.instructions.push(Instruction::move_register(copy_register, base_register));
                    let order: &[usize] = if arguments.len() == 3 { &[1, 0, 2] } else { &[0, 1, 2, 3] };
                    for &slot in order {
                        let (_, offset, pointee) = loads[slot];
                        let source = if slot == 0 { base_register } else { copy_register };
                        self.output.instructions.push(displacement_load(pointee, base_register + slot as u8, source, offset)?);
                    }
                    return Ok(());
                }
            }
        }
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for (index, argument) in arguments.iter().enumerate() {
            // A call argument whose float-ness does not match the parameter's needs an
            // int<->float conversion at the call site (the int->float magic-constant
            // sequence, or fctiwz). That conversion is not modeled, so defer rather than
            // place the argument in the wrong register file — passing an integer in r3 to a
            // float parameter that reads f1 (or vice versa) is a miscompile. A parameterless
            // / variadic position (no recorded type) keeps the argument-driven placement.
            if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                if matches!(parameter_type, Type::Float | Type::Double) != self.is_float_value(argument) {
                    return Err(Diagnostic::error("a call argument needs an int<->float conversion to match the parameter type (roadmap)"));
                }
            }
            if self.is_float_value(argument) {
                self.evaluate_float(argument, next_float)?;
                next_float += 1;
            } else {
                // A narrow (char/short) argument to a parameter that is NOT wider is passed
                // WITHOUT the int promotion — `void g(char); g(char_a)` is just `bl g`, no
                // `extsb` (only a wider parameter, e.g. `void g(int)`, widens the argument).
                // Handled for the in-place case (the value already sits in the argument
                // register); a move or a non-leaf falls through to the widening eval.
                if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                    if let Ok((register, width, _)) = self.leaf_info(argument) {
                        if width < 32 && (parameter_type.width() as u32) <= width as u32 && register == next_general {
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
                if let Some(parameter_type) = self.call_parameter_types.get(name).and_then(|types| types.get(index)) {
                    if (parameter_type.width() as u32) < 32 && constant_value(argument).is_none() {
                        let argument_is_narrow = match argument {
                            Expression::Variable(variable) if self.locations.contains_key(variable.as_str()) => {
                                self.leaf_info(argument).map(|(_, width, _)| width < 32).unwrap_or(false)
                            }
                            Expression::Variable(variable) => self.globals.get(variable.as_str()).map(|global| global.width() < 32).unwrap_or(false),
                            Expression::Dereference { pointer } => self.dereferenced_width(pointer).is_some_and(|width| width < 32),
                            Expression::Index { base, .. } => self.dereferenced_width(base).is_some_and(|width| width < 32),
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
                // A passthrough reuse like `f(x, x)` writes nothing for arg0, and
                // the single trailing `mr r4,r3` it produces is now hoisted into the
                // prologue slot — so the two-argument case is byte-exact. But three+
                // arguments (multiple trailing moves) or a computed trailing argument
                // need the full argument scheduler, so this still defers for now to
                // avoid emitting their unscheduled form.
                // A leaf argument already in its target register is a passthrough — no evaluation, so
                // it clobbers nothing and stays live for a later repeat's `mr` (`g(a, a)` is a in r3,
                // then `mr r4,r3`, the pre-copy hoisted into the prologue slot). Only for a 2-argument
                // call: 3+ arguments produce multiple trailing moves that need the full argument
                // scheduler, so those still defer via the clobber guard below.
                let passthrough_in_place = arguments.len() == 2
                    && self.leaf_info(argument).map(|(register, _, _)| register == next_general).unwrap_or(false);
                if !passthrough_in_place
                    && arguments[index + 1..].iter().any(|later| self.registers_used_by(later).contains(&next_general))
                {
                    return Err(Diagnostic::error("argument would clobber a register a later argument needs (roadmap)"));
                }
                self.evaluate_general(argument, next_general)?;
                next_general += 1;
            }
        }
        Ok(())
    }

}
