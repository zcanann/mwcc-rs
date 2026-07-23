//! Floating-point expression evaluation and multiply-add contraction.

use crate::analysis::*;
use crate::casts::IntToFloatSchedule;
use crate::generator::*;
use crate::operands::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};

impl Generator {
    /// Evaluate a float expression into float register `destination`.
    pub(crate) fn evaluate_float(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        match expression {
            Expression::IndexedUpdateValue { value } => {
                self.evaluate_float(value, destination)
            }
            Expression::BitFieldRead { .. } => Err(Diagnostic::error(
                "a promoted bit-field value is not a float value",
            )),
            Expression::CompoundLiteral { .. } => Err(Diagnostic::error(
                "a compound-literal argument needs the frame-temporary schedule (roadmap)",
            )),
            Expression::CallThrough { .. } => Err(Diagnostic::error(
                "an indirect call through a member function pointer is not supported here (captures only)",
            )),
            Expression::VirtualCall {
                object,
                vptr_offset,
                slot_offset,
                return_type,
                variadic,
                arguments,
            } => {
                if !matches!(return_type, Type::Float | Type::Double) {
                    return Err(Diagnostic::error(
                        "an integer virtual-call result used as a float needs conversion (roadmap)",
                    ));
                }
                self.emit_virtual_call(
                    object,
                    *vptr_offset,
                    *slot_offset,
                    *variadic,
                    arguments,
                    Some(destination),
                    true,
                )
            }
            Expression::AggregateLiteral(_) => Err(Diagnostic::error("an aggregate initializer is not supported here (captures only)")),
            Expression::PostStep { .. } => Err(Diagnostic::error(
                "a postfix step used as a float value is not supported yet (roadmap)",
            )),
            Expression::StringLiteral(_) => Err(Diagnostic::error("a string literal is not a float value")),
            Expression::Variable(name) => {
                // A frame-resident float is reloaded from its stack slot — but a
                // spilled PARAMETER whose slot was never written is still live in
                // its incoming register, and mwcc emits nothing (measured: the
                // writeback shapes reload, `*eptr = f(hx); return x;` does not).
                if let Some(slot) = self.frame_slots.get(name).copied() {
                    if slot.parameter_register == Some(destination) && !self.written_slots.contains(&slot.offset) {
                        return Ok(());
                    }
                    let instruction = if slot.size == 8 {
                        Instruction::LoadFloatDouble { d: destination, a: 1, offset: slot.offset }
                    } else {
                        Instruction::LoadFloatSingle { d: destination, a: 1, offset: slot.offset }
                    };
                    self.output.instructions.push(instruction);
                    return Ok(());
                }
                if self.locations.contains_key(name) {
                    let source = self.float_register_of(name)?;
                    if source != destination {
                        self.output.instructions.push(Instruction::FloatMove { d: destination, b: source });
                    }
                    Ok(())
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Member { base, offset, member_type, index_stride } => self.emit_member_load(base, *offset, *member_type, *index_stride, destination),
            Expression::MemberAddress { .. } => Err(Diagnostic::error("an array address is not a float value")),
            Expression::Assign { .. } => Err(Diagnostic::error("float assignment as an expression is not supported yet (roadmap)")),
            Expression::Comma { left, right } => {
                self.emit_comma_side_effect(left)?;
                self.evaluate_float(right, destination)
            }
            Expression::Index { base, index } => self.emit_subscript(base, index, destination),
            // `__fabs(x)` is an mwcc intrinsic that lowers to the single `fabs` instruction,
            // NOT an out-of-line call: `f64 fabs(f64 x) { return __fabs(x); }` -> `fabs f1,f1`.
            // A register leaf abs's in place from its own register; a memory load / sub-
            // expression goes through the scratch — the same operand placement as fneg.
            Expression::Call { name, arguments } if name == "__fabs" && arguments.len() == 1 => {
                let operand = &arguments[0];
                let source = if self.is_float_located(operand) {
                    self.emit_located_operand(operand, FLOAT_SCRATCH)?;
                    FLOAT_SCRATCH
                } else if is_complex(operand) {
                    if !fits_single_scratch(operand, true) {
                        return Err(Diagnostic::error("__fabs operand needs the full register allocator (roadmap M1)"));
                    }
                    self.evaluate_float(operand, FLOAT_SCRATCH)?;
                    FLOAT_SCRATCH
                } else {
                    self.float_register_of_leaf(operand)?
                };
                self.output.instructions.push(Instruction::FloatAbsolute { d: destination, b: source });
                Ok(())
            }
            // A call whose result is used as a float/double must actually RETURN float/double
            // (result in f1). A call returning int — or an implicitly-declared callee, which
            // defaults to `int` (the libm `w_*` wrappers: `double acos(double x){ return
            // __ieee754_acos(x); }` with no prototype) — leaves its result in r3, and mwcc
            // converts that r3 to double with the magic-bias sequence. That conversion of a
            // call RESULT (reusing the non-leaf frame, source already in r3) is not wired yet,
            // so defer rather than return the unconverted (garbage) float register. The
            // symmetric store case already defers this way in place_store_value.
            Expression::Call { name, arguments } => {
                if !matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)) {
                    return Err(Diagnostic::error("a call returning int used as a float needs an int->float conversion of the result (roadmap)"));
                }
                self.emit_call(name, arguments, Some(destination), true)
            }
            Expression::Binary { operator, left, right } => {
                let double = self.is_double_value(left) || self.is_double_value(right);
                // Mixed `int OP float` arithmetic: promote the integer operand to float first.
                if self.try_emit_mixed_promotion(*operator, left, right, destination, double)? {
                    return Ok(());
                }
                // A commutative float op (`+`/`*`) with a NEGATE operand diverges: `-a + b` keeps the
                // fneg but swaps the fadds operand order (mwcc puts the fneg result FIRST), and
                // `-(a*b) + c` contracts to a single `fnmsubs` we emit un-fused (fmuls; fneg; fadds).
                // Defer until those are modeled. A SUBTRACT (`-a - b`, `c - a*b`) and a bare negate keep
                // their byte-exact form, so this is gated to add/multiply with a direct negate operand.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply)
                    && (matches!(left.as_ref(), Expression::Unary { operator: UnaryOperator::Negate, .. })
                        || matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::Negate, .. }))
                {
                    return Err(Diagnostic::error("a float add/multiply with a negated operand needs fnmsubs / operand-order modeling (roadmap)"));
                }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_float_fused(*operator, left, right, destination, double)?
                {
                    return Ok(());
                }
                // `x / C` for a power-of-two constant >= 2 strength-reduces to a
                // multiply by the exact reciprocal: mwcc pools `1/C` and emits fmul(s).
                if *operator == BinaryOperator::Divide {
                    if let Expression::FloatLiteral(value) = right.as_ref() {
                        if let Some(reciprocal) = reciprocal_if_power_of_two(*value, double) {
                            let dividend = self.float_register_of_leaf(left)?;
                            if double {
                                self.load_double_constant(FLOAT_SCRATCH, reciprocal.to_bits());
                                self.output.instructions.push(Instruction::FloatMultiplyDouble { d: destination, a: dividend, c: FLOAT_SCRATCH });
                            } else {
                                self.load_float_constant(FLOAT_SCRATCH, reciprocal as f32);
                                self.output.instructions.push(Instruction::FloatMultiplySingle { d: destination, a: dividend, c: FLOAT_SCRATCH });
                            }
                            return Ok(());
                        }
                    }
                }
                // `E / E` for a structurally identical product ((x*y)/(x*y) —
                // e_fmod's NaN purge): mwcc CSEs the product into ONE compute
                // and divides it by itself (`fmul f0,x,y; fdiv d,f0,f0`), NOT
                // two independent multiplies. Gated to the probed shape — a
                // multiply of two register-resident variables (side-effect
                // free, so one evaluation is observably identical).
                if *operator == BinaryOperator::Divide
                    && structurally_equal(left, right)
                    && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: a, right: b }
                        if matches!(a.as_ref(), Expression::Variable(_)) && matches!(b.as_ref(), Expression::Variable(_)))
                {
                    self.evaluate_float(left, FLOAT_SCRATCH)?;
                    self.output.instructions.push(if double {
                        Instruction::FloatDivideDouble { d: destination, a: FLOAT_SCRATCH, b: FLOAT_SCRATCH }
                    } else {
                        Instruction::FloatDivideSingle { d: destination, a: FLOAT_SCRATCH, b: FLOAT_SCRATCH }
                    });
                    return Ok(());
                }
                // `f op f` for the identical side-effect-free MEMORY load (`*p + *p`, `a[i]*a[i]`, a
                // float global `gf * gf`): load ONCE into the scratch, then apply the op to that
                // register twice (`lfs f0,(p); fadds d,f0,f0`), like the integer identical-load idiom —
                // not two loads. A register-resident float parameter/local (`x + x`) is a free re-read,
                // so it is not routed here (only a global variable is a load).
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Multiply)
                    && same_operand(left, right)
                    && (matches!(left.as_ref(), Expression::Dereference { .. } | Expression::Member { .. } | Expression::Index { .. })
                        || matches!(left.as_ref(), Expression::Variable(name) if self.globals.contains_key(name.as_str())))
                {
                    self.evaluate_float(left, FLOAT_SCRATCH)?;
                    let r = FLOAT_SCRATCH;
                    self.output.instructions.push(match (operator, double) {
                        (BinaryOperator::Add, false) => Instruction::FloatAddSingle { d: destination, a: r, b: r },
                        (BinaryOperator::Add, true) => Instruction::FloatAddDouble { d: destination, a: r, b: r },
                        (BinaryOperator::Multiply, false) => Instruction::FloatMultiplySingle { d: destination, a: r, c: r },
                        _ => Instruction::FloatMultiplyDouble { d: destination, a: r, c: r },
                    });
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == FLOAT_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_float_operands(*operator, left, right, destination, double)?;
                self.output.instructions.push(float_combine(*operator, destination, operands, double)?);
                Ok(())
            }
            Expression::Unary { operator: UnaryOperator::Negate, operand } => {
                // -(-x) == x
                if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = operand.as_ref() {
                    return self.evaluate_float(inner, destination);
                }
                // A leaf negates in place; a memory load or sub-expression goes
                // through the scratch.
                let source = if self.is_float_located(operand) {
                    self.emit_located_operand(operand, FLOAT_SCRATCH)?;
                    FLOAT_SCRATCH
                } else if is_complex(operand) {
                    if !fits_single_scratch(operand, true) {
                        return Err(Diagnostic::error("float negation operand needs the full register allocator (roadmap M1)"));
                    }
                    self.evaluate_float(operand, FLOAT_SCRATCH)?;
                    FLOAT_SCRATCH
                } else {
                    self.float_register_of_leaf(operand)?
                };
                self.output.instructions.push(Instruction::FloatNegate { d: destination, b: source });
                Ok(())
            }
            Expression::Unary { .. } => Err(Diagnostic::error("only float negation is supported as a float unary")),
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } => {
                self.emit_float_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { operand, target_type } => self.emit_cast_to_float(operand, destination, *target_type == Type::Double),
            Expression::FloatLiteral(value) => {
                self.load_float_constant(destination, *value as f32);
                Ok(())
            }
            Expression::IntegerLiteral(_) => Err(Diagnostic::error("integer literal in float context")),
            Expression::AddressOf { .. } | Expression::ConstructedNew { .. } => {
                Err(Diagnostic::error("an address is not a float value"))
            }
        }
    }

    /// Mixed `int OP float` arithmetic (e.g. `int a + float b`): mwcc promotes the integer
    /// operand to float with the magic-constant idiom emitted into the scratch fp register
    /// (with the bias in f2, to avoid the live float operand in f1), then applies the float
    /// op. Handles only the verified shape — exactly one float-leaf operand and one int-width
    /// GPR leaf, the float operand already in f1 with the result also into f1 — so the bias
    /// register and operand placement are byte-exact; defers (`Ok(false)`) for anything else.
    fn try_emit_mixed_promotion(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        const FLOAT_FIRST: u8 = 1; // f1
        const BIAS_REGISTER: u8 = 2; // f2: avoids the scratch f0 and the operand/result f1
        if !matches!(
            operator,
            BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
        ) {
            return Ok(false);
        }
        let left_float_leaf = self.is_float_leaf(left);
        let right_float_leaf = self.is_float_leaf(right);
        if left_float_leaf != right_float_leaf {
            let (integer_operand, float_operand) = if left_float_leaf {
                (right, left)
            } else {
                (left, right)
            };
            let integer_is_wide = self
                .cast_operand_width(integer_operand)
                .is_none_or(|width| width >= 32);
            if integer_is_wide
                && self.general_register_of_leaf(integer_operand).is_ok()
                && self.float_register_of_leaf(float_operand).ok() == Some(FLOAT_FIRST)
                && destination == FLOAT_FIRST
            {
                self.emit_int_to_float(integer_operand, FLOAT_SCRATCH, double, BIAS_REGISTER)?;
                let (left_register, right_register) = if left_float_leaf {
                    (FLOAT_FIRST, FLOAT_SCRATCH)
                } else {
                    (FLOAT_SCRATCH, FLOAT_FIRST)
                };
                let operands = Operands::ordered(left_register, right_register)?;
                self.output.instructions.push(float_combine(
                    operator,
                    destination,
                    operands,
                    double,
                )?);
                return Ok(true);
            }
        }

        // A structured non-leaf body already owns conversion scratch space. It
        // may promote a loaded int beside a loaded float without introducing a
        // second frame: load both values, run the shared magic-bias body, then
        // combine in source order. The enclosing comparison scheduler can later
        // interleave two such conversions without duplicating their semantics.
        let left_float = self.is_float_operand(left);
        let right_float = self.is_float_operand(right);
        if left_float == right_float || !self.non_leaf || destination != FLOAT_SCRATCH {
            return Ok(false);
        }
        let (integer_operand, float_operand) = if left_float {
            (right, left)
        } else {
            (left, right)
        };
        if self
            .cast_operand_width(integer_operand)
            .is_some_and(|width| width < 32)
            || !(self.is_word_load(integer_operand)
                || self.general_register_of_leaf(integer_operand).is_ok())
            || !(self.is_float_operand(float_operand) || self.is_float_leaf(float_operand))
        {
            return Ok(false);
        }
        let integer_register = self.fresh_virtual_general();
        self.evaluate_general(integer_operand, integer_register)?;
        self.evaluate_float(float_operand, FLOAT_FIRST)?;
        let signed = self.signedness_of(integer_operand)?;
        self.emit_int_to_float_body(
            integer_register,
            FLOAT_SCRATCH,
            double,
            signed,
            BIAS_REGISTER,
            IntToFloatSchedule::LeafValue,
        );
        let (left_register, right_register) = if left_float {
            (FLOAT_FIRST, FLOAT_SCRATCH)
        } else {
            (FLOAT_SCRATCH, FLOAT_FIRST)
        };
        let operands = Operands::ordered(left_register, right_register)?;
        self.output
            .instructions
            .push(float_combine(operator, destination, operands, double)?);
        Ok(true)
    }

    /// Try to fuse `left op right` into a multiply-add when one side is a
    /// multiplication. mwcc contracts these under -fp_contract on, so we either
    /// fuse or stop honestly — never fall back to a separate multiply.
    pub(crate) fn try_emit_float_fused(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if !self.behavior.contract_floating_point {
            return Ok(false);
        }
        if let Some((x, y)) = as_multiplication(left) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(right)?;
            self.output.instructions.push(match (operator, double) {
                (BinaryOperator::Add, false) => Instruction::FloatMultiplyAddSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, false) => Instruction::FloatMultiplySubtractSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Add, true) => Instruction::FloatMultiplyAddDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, true) => Instruction::FloatMultiplySubtractDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        if let Some((x, y)) = as_multiplication(right) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(left)?;
            self.output.instructions.push(match (operator, double) {
                (BinaryOperator::Add, false) => Instruction::FloatMultiplyAddSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, false) => {
                    Instruction::FloatNegativeMultiplySubtractSingle {
                        d: destination,
                        a: multiplicand,
                        c: multiplier,
                        b: addend,
                    }
                }
                (BinaryOperator::Add, true) => Instruction::FloatMultiplyAddDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, true) => {
                    Instruction::FloatNegativeMultiplySubtractDouble {
                        d: destination,
                        a: multiplicand,
                        c: multiplier,
                        b: addend,
                    }
                }
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        Ok(false)
    }

    /// Whether a float-class expression is double-precision (so it uses the
    /// `fadd`/`fmul` family rather than the single `fadds`/`fmuls`). A double
    /// variable carries width 64; a binary op is double if either operand is.
    pub(crate) fn is_double_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(name) => match self.locations.get(name) {
                Some(location) => location.class == ValueClass::Float && location.width == 64,
                None => self.globals.get(name.as_str()) == Some(&Type::Double),
            },
            Expression::Binary { left, right, .. } => {
                self.is_double_value(left) || self.is_double_value(right)
            }
            Expression::Unary { operand, .. } => self.is_double_value(operand),
            Expression::Conditional {
                when_true,
                when_false,
                ..
            } => self.is_double_value(when_true) || self.is_double_value(when_false),
            Expression::Cast { target_type, .. } => *target_type == Type::Double,
            Expression::Member { member_type, .. } => *member_type == Type::Double,
            // A `double*` deref / subscript is a double value (so its arithmetic uses fadd/fmul, not
            // the single fadds/fmuls). Without this, double-pointer math read as single.
            Expression::Dereference { pointer } => {
                matches!(self.pointee_of(pointer), Ok(Pointee::Double))
            }
            Expression::Index { base, .. } => matches!(self.pointee_of(base), Ok(Pointee::Double)),
            Expression::Call { name, .. } => {
                self.call_return_types.get(name) == Some(&Type::Double)
            }
            _ => false,
        }
    }

    pub(crate) fn place_float_addend(&mut self, expression: &Expression) -> Compilation<u8> {
        // A memory-loaded addend (member, *float_ptr, or float global) goes through
        // the scratch, like a sub-expression.
        if self.is_float_located(expression) {
            self.emit_located_operand(expression, FLOAT_SCRATCH)?;
            return Ok(FLOAT_SCRATCH);
        }
        if is_complex(expression) {
            if !fits_single_scratch(expression, true) {
                return Err(Diagnostic::error(
                    "fused multiply-add addend needs the full register allocator (roadmap M1)",
                ));
            }
            self.evaluate_float(expression, FLOAT_SCRATCH)?;
            Ok(FLOAT_SCRATCH)
        } else {
            self.float_register_of_leaf(expression)
        }
    }

    /// Whether `operand` is a float value loaded from memory: a float struct
    /// member, a dereference of a float pointer, or a file-scope float global. Such
    /// an operand loads into a float register (its general base register, if any,
    /// is untouched).
    pub(crate) fn is_float_located(&self, operand: &Expression) -> bool {
        if let Some((_, _, member_type)) = as_member(operand) {
            return member_type == Type::Float;
        }
        if let Some(pointer) = as_dereference(operand) {
            return matches!(
                self.pointee_of(pointer),
                Ok(Pointee::Float | Pointee::Double)
            );
        }
        if let Expression::Variable(name) = operand {
            if !self.locations.contains_key(name) {
                return matches!(self.globals.get(name), Some(Type::Float | Type::Double));
            }
        }
        false
    }

    /// Place float operands when at least one is loaded from memory (a float member
    /// or `*float_pointer`). A single located operand loads into the scratch (its
    /// leaf partner stays home), two load left into the destination and right into
    /// the scratch, and a located-with-constant loads the constant first.
    fn place_float_located_operands(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<Operands> {
        const FLOAT_RESULT: u8 = 1;
        // A call result arrives in f1. For `loaded_value OP call()`, mwcc emits
        // the call first, then loads the memory operand into f0 so it does not
        // need to preserve that operand across the call. The mirrored source
        // spelling uses the same placement with the operand order reversed.
        if self.is_float_located(left) && self.is_float_call_value(right) {
            if !self.float_location_survives_call(left) {
                return Err(Diagnostic::error(
                    "a loaded float operand live across a call needs callee-saved base allocation (roadmap)",
                ));
            }
            self.evaluate_float(right, FLOAT_RESULT)?;
            self.emit_located_operand(left, FLOAT_SCRATCH)?;
            return Operands::ordered(FLOAT_SCRATCH, FLOAT_RESULT);
        }
        if self.is_float_call_value(left) && self.is_float_located(right) {
            if !self.float_location_survives_call(right) {
                return Err(Diagnostic::error(
                    "a loaded float operand live across a call needs callee-saved base allocation (roadmap)",
                ));
            }
            self.evaluate_float(left, FLOAT_RESULT)?;
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            return Operands::ordered(FLOAT_RESULT, FLOAT_SCRATCH);
        }
        if self.is_float_located(left) && self.is_float_located(right) {
            // The left load goes to a fresh virtual the allocator places (it
            // coalesces onto a free FPR, or the result register when that is free);
            // the right to the scratch. No longer needs a non-scratch result, so a
            // two-float-load sub-expression like `(*p + *q) * z` lowers.
            let anchor = self.fresh_virtual_float();
            self.emit_located_operand(left, anchor)?;
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            return Operands::ordered(anchor, FLOAT_SCRATCH);
        }
        if self.is_float_located(left) {
            if let Expression::FloatLiteral(value) = right {
                // The standalone form works (the constant loads into the result
                // register); as a sub-expression the outer operation reorders its
                // operands depending on the constant-folded inner (a scheduler
                // concern, Phase E), so defer rather than emit a non-matching order.
                if destination == FLOAT_SCRATCH {
                    return Err(Diagnostic::error(
                        "float load with constant needs a non-scratch destination (roadmap)",
                    ));
                }
                // A commutative op leads with the constant (into the dest), then the memory operand
                // (scratch): `lfs/lfd f1,const; lfs/lfd f0,(p); op f1,f1,f0`. A non-commutative op
                // (`value - const`, `value / const`) leads with the VALUE (into the dest), then the
                // constant (scratch): `lfs/lfd f1,(p); lfs/lfd f0,const; op f1,f1,f0`. load_float_literal
                // picks lfs vs lfd from `double`, so a double pointee loads an 8-byte constant.
                if matches!(operator, BinaryOperator::Subtract | BinaryOperator::Divide) {
                    self.emit_located_operand(left, destination)?;
                    self.load_float_literal(FLOAT_SCRATCH, *value, double);
                } else {
                    self.load_float_literal(destination, *value, double);
                    self.emit_located_operand(left, FLOAT_SCRATCH)?;
                }
                return Operands::ordered(destination, FLOAT_SCRATCH);
            }
            let right_register = self.float_register_of_leaf(right)?;
            self.emit_located_operand(left, FLOAT_SCRATCH)?;
            return Operands::ordered(FLOAT_SCRATCH, right_register);
        }
        if self.is_float_located(right) {
            if let Expression::FloatLiteral(value) = left {
                if destination == FLOAT_SCRATCH {
                    return Err(Diagnostic::error(
                        "float load with constant needs a non-scratch destination (roadmap)",
                    ));
                }
                self.load_float_literal(destination, *value, double);
                self.emit_located_operand(right, FLOAT_SCRATCH)?;
                return Operands::ordered(destination, FLOAT_SCRATCH);
            }
            let left_register = self.float_register_of_leaf(left)?;
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            return Operands::ordered(left_register, FLOAT_SCRATCH);
        }
        unreachable!("caller checked one side is a float load")
    }

    fn is_float_call_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Call { name, .. } => matches!(
                self.call_return_types.get(name),
                Some(Type::Float | Type::Double)
            ),
            Expression::VirtualCall { return_type, .. } => {
                matches!(return_type, Type::Float | Type::Double)
            }
            _ => false,
        }
    }

    fn float_location_survives_call(&self, expression: &Expression) -> bool {
        self.registers_used_by(expression)
            .into_iter()
            .all(|register| !matches!(register, 0 | 3..=12))
    }

    pub(crate) fn place_float_operands(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<Operands> {
        // A float operand loaded from memory (a member or `*float_pointer`) loads
        // into a float register; the general base register is untouched, so it can
        // even land straight in the float destination.
        if self.is_float_located(left) || self.is_float_located(right) {
            return self.place_float_located_operands(operator, left, right, destination, double);
        }
        // A literal paired with a computed operand needs two registers just as
        // two computed subtrees do. Evaluate the subtree into the result when
        // possible (otherwise a fresh virtual), then materialize the literal in
        // the scratch. Keeping the literal as the first commutative source
        // matches the ordinary literal/leaf path below while preserving source
        // order for subtraction and division.
        if let Expression::FloatLiteral(value) = left {
            if is_complex(right)
                || self.is_float_call_value(right)
                || matches!(right, Expression::Comma { .. } | Expression::Assign { .. })
            {
                let computed = if destination == FLOAT_SCRATCH {
                    self.fresh_virtual_float()
                } else {
                    destination
                };
                self.evaluate_float(right, computed)?;
                self.load_float_literal(FLOAT_SCRATCH, *value, double);
                return Operands::ordered(FLOAT_SCRATCH, computed);
            }
        }
        if let Expression::FloatLiteral(value) = right {
            if is_complex(left)
                || self.is_float_call_value(left)
                || matches!(left, Expression::Comma { .. } | Expression::Assign { .. })
            {
                let computed = if destination == FLOAT_SCRATCH {
                    self.fresh_virtual_float()
                } else {
                    destination
                };
                self.evaluate_float(left, computed)?;
                self.load_float_literal(FLOAT_SCRATCH, *value, double);
                return Operands::reversed(computed, FLOAT_SCRATCH);
            }
        }
        // A float constant operand is loaded from `.sdata2` into the scratch
        // register (an 8-byte `lfd` in a double op, a 4-byte `lfs` otherwise); the
        // other (leaf-variable) operand stays in place. mwcc emits the constant as
        // the first source of the (commutative) operation.
        if let Expression::FloatLiteral(value) = right {
            if matches!(left, Expression::Variable(_)) {
                let left_register = self.float_register_of_leaf(left)?;
                self.load_float_literal(FLOAT_SCRATCH, *value, double);
                return Operands::reversed(left_register, FLOAT_SCRATCH);
            }
        }
        if let Expression::FloatLiteral(value) = left {
            if matches!(right, Expression::Variable(_)) {
                let right_register = self.float_register_of_leaf(right)?;
                self.load_float_literal(FLOAT_SCRATCH, *value, double);
                return Operands::ordered(FLOAT_SCRATCH, right_register);
            }
        }
        let left_computed = is_complex(left)
            || self.is_float_call_value(left)
            || matches!(left, Expression::Comma { .. } | Expression::Assign { .. });
        let right_computed = is_complex(right)
            || self.is_float_call_value(right)
            || matches!(right, Expression::Comma { .. } | Expression::Assign { .. });
        if expression_has_call(right)
            && !left_computed
            && !self.float_location_survives_call(left)
        {
            return Err(Diagnostic::error(
                "a float leaf live across a right-hand call needs a callee-saved home",
            ));
        }
        if expression_has_call(left)
            && !right_computed
            && !self.float_location_survives_call(right)
        {
            return Err(Diagnostic::error(
                "a float leaf live across a left-hand call needs a callee-saved home",
            ));
        }
        match (left_computed, right_computed) {
            (false, false) => Operands::ordered(
                self.float_register_of_leaf(left)?,
                self.float_register_of_leaf(right)?,
            ),
            (true, false) => {
                self.evaluate_float(left, FLOAT_SCRATCH)?;
                Operands::reversed(FLOAT_SCRATCH, self.float_register_of_leaf(right)?)
            }
            (false, true) => {
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Operands::ordered(self.float_register_of_leaf(left)?, FLOAT_SCRATCH)
            }
            (true, true) => {
                // The left side computes into a fresh float virtual the allocator
                // places (keeping the right's inputs live); the right into the
                // scratch. (Sethi-Ullman heavier-first — as done for the integer
                // case — gets the float *registers* right here but not the order:
                // mwcc emits the lighter operand's add before the heavier product,
                // a scheduler nuance not yet pinned down, so left-to-right stands.)
                let temp = self.with_reserved_inputs(right, |generator| {
                    let temp = generator.fresh_virtual_float();
                    generator.evaluate_float(left, temp)?;
                    Ok(temp)
                })?;
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Operands::ordered(temp, FLOAT_SCRATCH)
            }
        }
    }
}

/// If `value` is a power-of-two constant `>= 2` in the given precision, return
/// its exact reciprocal, so `x / value` becomes `x * reciprocal`. mwcc reduces
/// `/2`, `/4`, `/8`, … this way but keeps `fdiv` for fractional powers of two
/// (`/0.5`) and non-powers.
fn reciprocal_if_power_of_two(value: f64, double: bool) -> Option<f64> {
    if !(value >= 2.0) || !value.is_finite() {
        return None;
    }
    let is_power_of_two = if double {
        value.to_bits() & 0x000F_FFFF_FFFF_FFFF == 0
    } else {
        (value as f32).to_bits() & 0x007F_FFFF == 0
    };
    is_power_of_two.then(|| 1.0 / value)
}
