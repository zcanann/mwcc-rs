//! C++ scalar allocation followed by null-guarded construction.

use super::*;

impl Generator {
    /// Emit the CodeWarrior EABI sequence for `new Class(args...)` or placement
    /// new: evaluate the address producer once, retain/test the resulting
    /// pointer, and invoke the constructor only when it is non-null.
    pub(crate) fn emit_constructed_new(
        &mut self,
        allocation: &Expression,
        allocation_size: u32,
        constructor: &str,
        arguments: &[Expression],
        destination: u8,
    ) -> Compilation<()> {
        self.emit_constructed_new_impl(
            allocation,
            allocation_size,
            constructor,
            arguments,
            Some(destination),
        )
    }

    pub(crate) fn emit_discarded_constructed_new(
        &mut self,
        allocation: &Expression,
        allocation_size: u32,
        constructor: &str,
        arguments: &[Expression],
    ) -> Compilation<()> {
        self.emit_constructed_new_impl(
            allocation,
            allocation_size,
            constructor,
            arguments,
            None,
        )
    }

    fn emit_constructed_new_impl(
        &mut self,
        allocation: &Expression,
        allocation_size: u32,
        constructor: &str,
        arguments: &[Expression],
        destination: Option<u8>,
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
            destination.unwrap_or(0)
        } else {
            0
        });
        let done = self.fresh_label();
        let direct_discarded_placement = if destination.is_none() && inline_body.is_some() {
            match allocation {
                Expression::Variable(name) => self.lookup_general(name),
                _ => None,
            }
        } else {
            None
        };
        if let Some(source) = direct_discarded_placement {
            // Placement new used only for its construction side effects keeps
            // the source object in place. MWCC records the null test through
            // its r0 scratch, branches, then promotes the non-null pointer to
            // the callee-saved home used by the inlined constructor graph.
            let tested = self.fresh_virtual_general_preferring(0);
            self.output.instructions.push(Instruction::OrRecord {
                a: tested,
                s: source,
                b: source,
            });
            self.emit_branch_conditional_to(12, 2, done); // beq: placement address is null
            self.output
                .instructions
                .push(Instruction::move_register(retained, tested));
        } else {
            self.evaluate_general(allocation, Eabi::FIRST_GENERAL_ARGUMENT)?;
            self.output.instructions.push(Instruction::OrRecord {
                a: retained,
                s: Eabi::FIRST_GENERAL_ARGUMENT,
                b: Eabi::FIRST_GENERAL_ARGUMENT,
            });
            self.emit_branch_conditional_to(12, 2, done); // beq: allocation failed
        }

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
            let emitted = if self.try_emit_constructor_initializer_run(&inline_body, retained)? {
                Ok(())
            } else {
                self.emit_comma_side_effect(&inline_body)
            };
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
        if let Some(destination) = destination {
            if destination != retained {
                self.output
                    .instructions
                    .push(Instruction::move_register(destination, retained));
            }
        }
        Ok(())
    }
}
