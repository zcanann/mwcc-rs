//! C++ scalar allocation followed by null-guarded construction.

use super::*;

impl Generator {
    /// Emit the CodeWarrior EABI sequence for `new Class(args...)`:
    /// allocate once, retain/test the returned pointer, and invoke the
    /// constructor only for a non-null allocation result.
    pub(crate) fn emit_constructed_new(
        &mut self,
        allocation_size: u32,
        constructor: &str,
        arguments: &[Expression],
        destination: u8,
    ) -> Compilation<()> {
        if arguments.len() > usize::from(Eabi::LAST_GENERAL_ARGUMENT - 3) {
            return Err(Diagnostic::error(
                "constructed C++ new has too many general constructor arguments",
            ));
        }
        if arguments
            .iter()
            .any(|argument| self.is_float_value(argument) || expression_has_call(argument))
        {
            return Err(Diagnostic::error(
                "constructed C++ new with floating or call-bearing arguments needs the constructor argument scheduler (roadmap)",
            ));
        }

        self.emit_call(
            "__nw__FUl",
            &[Expression::IntegerLiteral(i64::from(allocation_size))],
            None,
            false,
        )?;

        // MWCC uses r0 for the short-lived null-or-constructed result when no
        // callee-saved value competes with it; the allocator may override this
        // preference when surrounding liveness requires another home.
        let retained = self.fresh_virtual_general_preferring(0);
        self.output.instructions.push(Instruction::OrRecord {
            a: retained,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done); // beq: allocation failed

        for (index, argument) in arguments.iter().enumerate() {
            self.evaluate_general(argument, Eabi::FIRST_GENERAL_ARGUMENT + 1 + index as u8)?;
        }
        self.emit_call(constructor, &[], None, false)?;
        self.output.instructions.push(Instruction::move_register(
            retained,
            Eabi::general_result().number,
        ));

        self.bind_label(done);
        if destination != retained {
            self.output
                .instructions
                .push(Instruction::move_register(destination, retained));
        }
        Ok(())
    }
}
