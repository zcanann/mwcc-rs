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
        // Two word members of one pointer base, where loading the first clobbers the
        // base register (`g(p->a, p->b)` with `p` in r3): mwcc pre-copies the base to
        // the second argument register, then loads each member —
        // `mr r4,r3; lwz r3,off0(r3); lwz r4,off1(r4)`. The pre-copy `mr` is hoisted
        // into the non-leaf prologue slot by the body emitter. (The general N-member
        // / mixed-width choreography is the allocator's; this handles the 2-word case.)
        if let [Expression::Member { base: base0, offset: offset0, member_type: type0, index_stride: None },
                Expression::Member { base: base1, offset: offset1, member_type: type1, index_stride: None }] = arguments
        {
            if let (Expression::Variable(pointer0), Expression::Variable(pointer1)) = (base0.as_ref(), base1.as_ref()) {
                let base_register = Eabi::FIRST_GENERAL_ARGUMENT;
                let copy_register = base_register + 1;
                let is_word = |member: Type| matches!(member, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. });
                if pointer0 == pointer1
                    && is_word(*type0)
                    && is_word(*type1)
                    && self.locations.get(pointer0.as_str()).map(|location| location.register) == Some(base_register)
                {
                    if let (Some(pointee0), Some(pointee1)) = (pointee_of_type(*type0), pointee_of_type(*type1)) {
                        self.output.instructions.push(Instruction::move_register(copy_register, base_register));
                        self.output.instructions.push(displacement_load(pointee0, base_register, base_register, *offset0 as i16)?);
                        self.output.instructions.push(displacement_load(pointee1, copy_register, copy_register, *offset1 as i16)?);
                        return Ok(());
                    }
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
