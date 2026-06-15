//! Integer<->float conversions.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Type};
use crate::generator::*;

impl Generator {

    /// Emit a cast of an integer operand to a float in `destination` — mwcc's
    /// magic-constant conversion: bias the integer (flip its sign bit), assemble
    /// the double `0x43300000_<biased int>` on the stack, and subtract the bias
    /// `0x4330000000000000`. The bias double lives in `.sdata2`; the `lfd dest,0(0)`
    /// is byte-correct here, but its `R_PPC_EMB_SDA21` relocation and the constant
    /// pool are the next M3 step. Leaf integer operands only.
    pub(crate) fn emit_cast_to_float(&mut self, operand: &Expression, destination: u8) -> Compilation<()> {
        // The conversion assembles `0x43300000_<int>` on the stack and subtracts a
        // magic bias double (pooled in `.sdata2`). A signed value flips its sign bit
        // first and subtracts `0x43300000_80000000`; an unsigned value skips the
        // flip and subtracts `0x43300000_00000000`. Either bumps the @N counter.
        let signed = self.signedness_of(operand)?;
        let bias: u64 = if signed { 0x4330_0000_8000_0000 } else { 0x4330_0000_0000_0000 };
        let source = self.general_register_of_leaf(operand)?;
        self.output.has_conversion = true;
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        if signed {
            self.output.instructions.push(Instruction::XorImmediateShifted { a: source, s: source, immediate: 0x8000 });
        }
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200)); // lis r0, 0x4330
        // The bias load and the value store are independent; builds schedule them
        // in opposite orders (GC/2.0p1 stores first, every other build loads first).
        if self.behavior.float_cast_value_store_first {
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
            self.load_double_constant(destination, bias);
        } else {
            self.load_double_constant(destination, bias);
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: FLOAT_SCRATCH, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: destination, a: FLOAT_SCRATCH, b: destination });
        Ok(())
    }

    /// Emit a cast of a float operand to an integer in `destination`. mwcc
    /// converts with `fctiwz`, then bounces the value through the stack frame.
    /// Leaf float operands only for now; int->float (the constant-pool direction)
    /// is handled separately once .sdata2 lands.
    pub(crate) fn emit_cast_to_integer(&mut self, target_type: Type, operand: &Expression, destination: u8) -> Compilation<()> {
        if self.is_float_leaf(operand) {
            // float -> int: convert, bounce through the frame, then narrow if needed.
            let source = self.float_register_of_leaf(operand)?;
            self.output.has_conversion = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: FLOAT_SCRATCH, b: source });
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::StoreFloatDouble { s: FLOAT_SCRATCH, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: 12 });
            if target_type.width() < 32 {
                self.emit_widen(destination, destination, target_type.width(), self.signed_of(target_type));
            }
            return Ok(());
        }
        // int -> int narrowing: place the operand (sub-expression -> scratch),
        // then extend/truncate to the target width into the destination.
        if target_type.width() < 32 {
            let Some(source) = self.place_operand(operand, destination, false)? else {
                return Err(Diagnostic::error("cast operand needs the full register allocator (roadmap M1)"));
            };
            self.emit_widen(destination, source, target_type.width(), self.signed_of(target_type));
        } else {
            self.evaluate_general(operand, destination)?;
        }
        Ok(())
    }
}
