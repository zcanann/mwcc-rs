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
}
