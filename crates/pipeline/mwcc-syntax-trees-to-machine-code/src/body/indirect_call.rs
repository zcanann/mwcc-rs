//! Bare indirect-call statements through a memory-resident function pointer with constant
//! arguments: `(*s->fp)(k)`, `(**pp)(k)`. The no-argument form lives in `emit_statement`; this
//! module handles the argument case, where the pointer's base collides with the argument
//! registers and mwcc copies it out to r4 before loading the callee.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A bare indirect call through a MEMORY-resident function pointer, passing small integer
    /// constants: `void f(struct S *s){ s->cb(7); }` / `void f(VF *pp){ (**pp)(7); }`. The callee
    /// address lives at `off(param)`; its base sits in r3, colliding with the first argument, so
    /// mwcc copies the base to r4, materializes the first argument (`li r3,c0`), saves the link
    /// register (latency-filled into the mflr gap), loads the callee (`lwz r12,off(r4)`), then
    /// materializes the remaining arguments (r4 is free again) and `mtctr r12; bctrl`:
    ///
    /// ```text
    ///   stwu; mflr r0; mr r4,r3; li r3,c0; stw r0,20; lwz r12,off(r4); li r4,c1; …; mtctr; bctrl
    /// ```
    ///
    /// Only a single pointer parameter as the base and all-constant arguments are modeled; a
    /// computed/parameter argument, a non-parameter base, or a returned result defers (unmeasured).
    pub(crate) fn try_indirect_call_with_constant_args(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function.parameters.len() != 1
        {
            return Ok(false);
        }
        // The body is exactly one bare indirect call.
        let [Statement::Expression(Expression::CallThrough { target, arguments })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if arguments.is_empty() || arguments.len() > 8 {
            return Ok(false);
        }
        // The callee address is `off(param)`: either `*param` (offset 0) or `param->member`.
        let parameter_name = &function.parameters[0].name;
        let offset = match target.as_ref() {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) if name == parameter_name => 0i16,
                _ => return Ok(false),
            },
            Expression::Member { base, offset, .. } => match base.as_ref() {
                Expression::Variable(name) if name == parameter_name => *offset as i16,
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        // Every argument is a small integer constant.
        let mut constants = Vec::with_capacity(arguments.len());
        for argument in arguments {
            match argument {
                Expression::IntegerLiteral(value)
                    if (i16::MIN as i64..=i16::MAX as i64).contains(value) =>
                {
                    constants.push(*value as i16);
                }
                _ => return Ok(false),
            }
        }
        // Sanity: the base parameter arrives in r3 (a general register).
        match self.locations.get(parameter_name) {
            Some(location) if location.class == ValueClass::General && location.register == 3 => {}
            _ => return Ok(false),
        }

        // The base register r3 collides with the first argument, so it is copied to r4 and the
        // callee is loaded from there AFTER the first argument and the link-register save (which
        // fills the mflr latency gap). Emitting this pre-scheduled keeps the passes off it: `mflr`
        // is not immediately followed by the save, so the link-register scheduler leaves it be.
        self.non_leaf = true;
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::move_register(4, 3)); // mr r4,r3
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: constants[0],
        }); // li r3,c0
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        }); // stw r0,20
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 4,
            offset,
        }); // lwz r12,off(r4)
        for (index, &value) in constants.iter().enumerate().skip(1) {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3 + index as u8,
                a: 0,
                immediate: value,
            });
        }
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
