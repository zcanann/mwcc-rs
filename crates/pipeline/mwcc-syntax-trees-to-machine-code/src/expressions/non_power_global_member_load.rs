//! Multiply-first addressing for non-power-of-two global record strides.
//!
//! Scaling consumes the index before the global base can reuse its register.
//! A scratch or floating-point result needs a separate GPR address, since r0
//! denotes zero as a displacement base and FPR numbers are a different bank.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn emit_non_power_global_member_load(
        &mut self,
        name: &str,
        total_size: u32,
        index: &Expression,
        stride: u32,
        offset: u32,
        pointee: Pointee,
        destination: u32,
    ) -> Compilation<()> {
        let index_register = self.materialize_index_operand(index)?;
        if let Ok(immediate) = i16::try_from(stride) {
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: GENERAL_SCRATCH,
                    a: index_register.into(),
                    immediate,
                });
        } else {
            let factor = self.fresh_virtual_general();
            self.load_integer_constant(factor, i64::from(stride));
            self.output.instructions.push(Instruction::MultiplyLow {
                d: GENERAL_SCRATCH,
                a: index_register.into(),
                b: factor,
            });
        }
        let address = if destination == GENERAL_SCRATCH
            || matches!(pointee, Pointee::Float | Pointee::Double)
        {
            self.fresh_virtual_general()
        } else {
            destination
        };
        self.emit_global_array_base(name, total_size, address)?;
        if offset == 0 {
            self.output.instructions.push(indexed_load(
                pointee,
                destination,
                address.into(),
                GENERAL_SCRATCH,
            )?);
        } else {
            self.output.instructions.push(Instruction::Add {
                d: address,
                a: address,
                b: GENERAL_SCRATCH,
            });
            let displacement = self.emit_member_base_adjustment(address, offset);
            self.output.instructions.push(displacement_load(
                pointee,
                destination,
                address,
                displacement,
            )?);
        }
        Ok(())
    }
}
