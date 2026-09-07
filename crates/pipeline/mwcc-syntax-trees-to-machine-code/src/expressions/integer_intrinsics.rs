//! Instruction selection for integer intrinsics that retain call-shaped syntax.

use super::*;

impl Generator {
    pub(crate) fn try_emit_integer_intrinsic(
        &mut self,
        name: &str,
        arguments: &[Expression],
        destination: u8,
    ) -> Compilation<bool> {
        if !is_integer_intrinsic_call(name, arguments.len()) {
            return Ok(false);
        }
        if crate::intrinsics::classify(name, arguments.len())
            == Some(crate::intrinsics::Intrinsic::RotateLeftWordInsert)
        {
            return self
                .emit_rotate_left_word_insert(arguments, destination)
                .map(|()| true);
        }
        let operand = &arguments[0];
        let source = match self.general_register_of_leaf(operand) {
            Ok(source) => source,
            Err(_) if destination == GENERAL_SCRATCH => {
                self.evaluate_general(operand, destination)?;
                destination
            }
            Err(_) => {
                let source = self.fresh_virtual_general();
                self.evaluate_general(operand, source)?;
                source
            }
        };
        // Leaf schedules use r0 for the sign mask. In a non-leaf allocation the
        // ordinary volatile-register order wins instead (WENC selects r5), while
        // a scratch-resident operand naturally makes the r0 preference spill to
        // its newly dead input register.
        let sign = if self.non_leaf {
            self.fresh_virtual_general()
        } else {
            self.fresh_virtual_general_preferring(GENERAL_SCRATCH)
        };
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: sign,
                s: source,
                shift: 31,
            });
        self.output.instructions.push(Instruction::Xor {
            a: destination,
            s: sign,
            b: source,
        });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: destination,
            a: sign,
            b: destination,
        });
        Ok(true)
    }

    fn emit_rotate_left_word_insert(
        &mut self,
        arguments: &[Expression],
        destination: u8,
    ) -> Compilation<()> {
        let operands = crate::intrinsics::rotate_insert(arguments).ok_or_else(|| {
            Diagnostic::error("__rlwimi requires constant shift and mask operands in 0..31")
        })?;
        // The insert reads its old destination. Separate virtual identities
        // preserve both inputs even when the requested result aliases one of
        // them or computing the source needs scratch registers or a call.
        let initial = if destination == GENERAL_SCRATCH {
            self.fresh_virtual_general_preferring(GENERAL_SCRATCH)
        } else {
            self.fresh_virtual_general()
        };
        self.with_reserved_inputs(operands.source, |me| {
            me.evaluate_general(operands.initial, initial)
        })?;
        let source = self.fresh_virtual_general();
        let restore = self.reserved.insert(initial);
        let evaluated = self.evaluate_general(operands.source, source);
        if restore {
            self.reserved.remove(&initial);
        }
        evaluated?;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: initial,
                s: source,
                shift: operands.shift,
                begin: operands.begin,
                end: operands.end,
            });
        self.output
            .instructions
            .push(Instruction::move_register(destination, initial));
        Ok(())
    }
}
