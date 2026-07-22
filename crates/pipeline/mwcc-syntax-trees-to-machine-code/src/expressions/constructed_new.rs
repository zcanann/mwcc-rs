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

        let inline_result_name = format!("__mwcc_constructed_new_{}", self.next_virtual);
        let inline_body = self.inline_bodies.expand_constructed_new_body(
            constructor,
            &inline_result_name,
            arguments,
        );
        // An inlined constructor leaves the allocation in its consumer's home.
        // An out-of-line constructor still uses r3 for `this` and its return, so
        // retain the old r0 preference until the call completes.
        let retained = self.fresh_virtual_general_preferring(if inline_body.is_some() {
            destination
        } else {
            0
        });
        self.output.instructions.push(Instruction::OrRecord {
            a: retained,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done); // beq: allocation failed

        if let Some(inline_body) = inline_body {
            self.locations.insert(
                inline_result_name.clone(),
                crate::generator::Location {
                    class: crate::generator::ValueClass::General,
                    register: retained,
                    signed: false,
                    width: 32,
                    pointee: None,
                    stride: Some(allocation_size),
                },
            );
            let emitted = self.emit_comma_side_effect(&inline_body);
            self.locations.remove(&inline_result_name);
            emitted?;
        } else {
            for (index, argument) in arguments.iter().enumerate() {
                self.evaluate_general(argument, Eabi::FIRST_GENERAL_ARGUMENT + 1 + index as u8)?;
            }
            self.emit_call(constructor, &[], None, false)?;
            self.output.instructions.push(Instruction::move_register(
                retained,
                Eabi::general_result().number,
            ));
        }

        self.bind_label(done);
        if destination != retained {
            self.output
                .instructions
                .push(Instruction::move_register(destination, retained));
        }
        Ok(())
    }
}
