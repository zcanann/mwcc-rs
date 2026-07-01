//! Integer<->float conversions.

use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Type};
use crate::generator::*;

impl Generator {

    /// The integer width (bits) of a cast's leaf operand, when determinable. Used to
    /// defer a cast-to-float of a narrow (char/short) value: mwcc first widens it to
    /// int (extsb/extsh) and reschedules the magic-constant idiom around that extra
    /// instruction — a sequence not modeled here. `None` (unknown) proceeds as before.
    pub(crate) fn cast_operand_width(&self, operand: &Expression) -> Option<u32> {
        match operand {
            Expression::Variable(name) => self
                .locations
                .get(name)
                .map(|location| location.width as u32)
                .or_else(|| self.globals.get(name).map(|global_type| global_type.width() as u32)),
            Expression::Member { member_type, .. } => Some(member_type.width() as u32),
            Expression::Cast { target_type, .. } => Some(target_type.width() as u32),
            Expression::Dereference { pointer } => self.pointee_of(pointer).ok().map(|pointee| pointee.element().width() as u32),
            Expression::Index { base, .. } => self.pointee_of(base).ok().map(|pointee| pointee.element().width() as u32),
            _ => None,
        }
    }

    /// Emit a cast of an integer operand to a float in `destination` — mwcc's
    /// magic-constant conversion: bias the integer (flip its sign bit), assemble
    /// the double `0x43300000_<biased int>` on the stack, and subtract the bias
    /// `0x4330000000000000`. The bias double lives in `.sdata2`; the `lfd dest,0(0)`
    /// is byte-correct here, but its `R_PPC_EMB_SDA21` relocation and the constant
    /// pool are the next M3 step. Leaf integer operands only.
    pub(crate) fn emit_cast_to_float(&mut self, operand: &Expression, destination: u8, double: bool) -> Compilation<()> {
        // A cast between floating types needs an instruction only when it NARROWS:
        // `(float)` of a double rounds it to single precision with `frsp`. A leaf
        // rounds in place from its own register; a sub-expression is computed into
        // the destination first (mwcc keeps that intermediate in the destination,
        // not the scratch), then rounded `frsp d, d`. A same-width `(double)` of a
        // double is a NO-OP — the value only needs to land in `destination` (mwcc
        // emits nothing when it is already there, e.g. `return (double)dbl_call()`
        // whose result is already in the return register); do NOT emit a spurious frsp.
        if self.is_double_value(operand) {
            if self.is_float_leaf(operand) {
                let source = self.float_register_of_leaf(operand)?;
                if double {
                    if source != destination {
                        self.output.instructions.push(Instruction::FloatMove { d: destination, b: source });
                    }
                } else {
                    self.output.instructions.push(Instruction::RoundToSingle { d: destination, b: source });
                }
            } else {
                self.evaluate_float(operand, destination)?;
                if !double {
                    self.output.instructions.push(Instruction::RoundToSingle { d: destination, b: destination });
                }
            }
            return Ok(());
        }
        // A narrow integer (char/short) cast to float is first widened to int with
        // extsb/extsh, and mwcc reschedules the magic-constant idiom around that extra
        // instruction. That sequence is not modeled, so defer rather than emit the
        // int-width idiom unextended (wrong bytes for a negative char/short).
        if self.cast_operand_width(operand).map_or(false, |width| width < 32) {
            return Err(mwcc_core::Diagnostic::error("cast-to-float of a narrow (char/short) value is not modeled (roadmap)"));
        }
        // The magic bias goes in a register distinct from the assembled value's f0
        // (FLOAT_SCRATCH): the destination when it isn't f0 (a return into f1), else f1
        // for a value/store into f0 — otherwise the assembled `lfd f0` would overwrite
        // the bias, leaving `fsub f0,f0,f0` = 0.
        const FLOAT_FIRST: u8 = 1; // f1
        let bias_register = if destination != FLOAT_SCRATCH { destination } else { FLOAT_FIRST };
        self.emit_int_to_float(operand, destination, double, bias_register)
    }

    /// The magic-constant int->float idiom into `destination`, with the bias double held in
    /// `bias_register` (caller-chosen so a mixed-arithmetic promotion can place the bias in a
    /// register that avoids the live float operand). Assembles `0x43300000_<biased int>` on the
    /// frame and subtracts the `0x4330..` bias. The operand is an int-width GPR leaf.
    pub(crate) fn emit_int_to_float(&mut self, operand: &Expression, destination: u8, double: bool, bias_register: u8) -> Compilation<()> {
        // A signed value flips its sign bit first and subtracts `0x43300000_80000000`; an
        // unsigned value skips the flip and subtracts `0x43300000_00000000`. Bumps the @N counter.
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
        // The bias load and the value store are independent; builds schedule them in
        // opposite orders (GC/2.0p1 stores first, every other build loads first).
        if self.behavior.float_cast_value_store_first {
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
            self.load_double_constant(bias_register, bias);
        } else {
            self.load_double_constant(bias_register, bias);
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: FLOAT_SCRATCH, a: 1, offset: 8 });
        // The bias subtract yields the result at the requested precision: `fsub`
        // for an int->double conversion, `fsubs` for int->float.
        self.output.instructions.push(if double {
            Instruction::FloatSubtractDouble { d: destination, a: FLOAT_SCRATCH, b: bias_register }
        } else {
            Instruction::FloatSubtractSingle { d: destination, a: FLOAT_SCRATCH, b: bias_register }
        });
        Ok(())
    }

    /// Emit a cast of a float operand to an integer in `destination`. mwcc
    /// converts with `fctiwz`, then bounces the value through the stack frame.
    /// Leaf float operands only for now; int->float (the constant-pool direction)
    /// is handled separately once .sdata2 lands.
    pub(crate) fn emit_cast_to_integer(&mut self, target_type: Type, operand: &Expression, destination: u8) -> Compilation<()> {
        // `(int)(float)x` / `(int)(double)x` is a ROUND-TRIP conversion, not an identity — a float
        // cannot represent every int exactly, so the value can change. The full int->float->int
        // sequence (constant-pool magic in, fctiwz out) is not modeled for a cast operand, so
        // defer rather than fall through to the integer path, which would cancel both casts and
        // silently drop the conversion (returning x unchanged — a miscompile for large ints).
        if matches!(operand, Expression::Cast { target_type: Type::Float | Type::Double, .. }) {
            return Err(mwcc_core::Diagnostic::error("an int<-float<-int round-trip cast is not modeled (roadmap)"));
        }
        if self.is_float_leaf(operand) {
            // float -> unsigned uses a runtime helper call (the value may exceed
            // INT_MAX, which `fctiwz` cannot represent), not the signed frame bounce.
            if !self.signed_of(target_type) {
                return Err(mwcc_core::Diagnostic::error("float-to-unsigned conversion needs a runtime helper (roadmap)"));
            }
            // float -> int: convert, bounce through the frame, then narrow if needed.
            let source = self.float_register_of_leaf(operand)?;
            self.output.has_conversion = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: FLOAT_SCRATCH, b: source });
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::StoreFloatDouble { s: FLOAT_SCRATCH, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: 12 });
            if target_type.width() < 32 {
                // mwcc does NOT narrow a float -> (char/short) cast with an extend
                // instruction: `return (char)a` leaves the fctiwz int in r3 as-is, and a
                // store truncates via stb/sth. Emitting an extsb/extsh here is a spurious
                // extra instruction; the exact contexts where mwcc does vs does not narrow
                // are not modeled, so defer rather than diff.
                return Err(mwcc_core::Diagnostic::error("float-to-narrow-int cast narrowing is not modeled (roadmap)"));
            }
            return Ok(());
        }
        // A float operand that is NOT a leaf — a global (`(int)gf`), a load, a member,
        // or a float-returning CALL (`(int)hf()`) — needs the same fctiwz + frame-bounce
        // but with the value loaded/called first (mwcc's `bl hf; fctiwz f0,f1; ...`).
        // Until that is modeled, defer: falling through to the integer path below would
        // evaluate the float operand into a general register and store garbage. (A call
        // would call with `float_result=false` and store the untouched r3.)
        let is_float_call = matches!(operand, Expression::Call { name, .. }
            if matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)));
        if self.is_float_value(operand) || self.is_float_operand(operand) || is_float_call {
            return Err(mwcc_core::Diagnostic::error("float-to-int of a non-leaf operand needs the load/call + convert path (roadmap)"));
        }
        // `(unsigned char)<char load>`: the byte load (`lbz`/`lbzx`) already zero-extends to
        // 0..255, which IS the unsigned-char value, so mwcc drops BOTH the signed-promotion
        // extsb and the cast's clrlwi — `(unsigned char)gc` / `(unsigned char)*p` is a bare
        // `lbz`. Emit just the load (raw, no promotion) with no trailing widen. A signed-char
        // global, dereference, member, or array element qualifies; a short operand needs the
        // `& 0xff` (its load is wider), and a leaf is handled byte-exactly by the path below.
        let operand_is_char_load = self.is_signed_byte_load(operand)?
            || matches!(operand, Expression::Variable(name)
                if !self.locations.contains_key(name.as_str()) && self.globals.get(name.as_str()) == Some(&Type::Char));
        if target_type == Type::UnsignedChar && operand_is_char_load {
            let saved_truncation_context = self.narrow_truncation_context;
            self.narrow_truncation_context = true;
            let evaluated = self.evaluate_general(operand, destination);
            self.narrow_truncation_context = saved_truncation_context;
            evaluated?;
            return Ok(());
        }
        // int -> int narrowing: place the operand (sub-expression -> scratch),
        // then extend/truncate to the target width into the destination.
        if target_type.width() < 32 {
            // The cast itself narrows (extsb/extsh/clrlwi), so a leaf param/local operand is
            // read RAW — it skips the promotion extsb that the cast's own widen would
            // immediately override: `(unsigned char)a` is `clrlwi r3,r3,24`, not `extsb r0,r3;
            // clrlwi r3,r0,24`, and `(char)char_a` is one `extsb`, not two. A pointer load
            // (`(unsigned char)*p`) keeps its char-load defer (raw-reading it would expose the
            // load's r0-vs-destination register choice — a byte diff). A char GLOBAL is also
            // excluded: mwcc recognizes its `lbz` already zero-extends and drops the cast
            // entirely (`(unsigned char)gc` is a bare `lbz`), a separate fold not modeled here.
            let saved_truncation_context = self.narrow_truncation_context;
            if matches!(operand, Expression::Variable(name) if self.locations.contains_key(name.as_str())) {
                self.narrow_truncation_context = true;
            }
            let source = self.place_operand_or_scratch(operand, destination);
            self.narrow_truncation_context = saved_truncation_context;
            let source = source?;
            self.emit_widen(destination, source, target_type.width(), self.signed_of(target_type));
        } else {
            self.evaluate_general(operand, destination)?;
        }
        Ok(())
    }
}
