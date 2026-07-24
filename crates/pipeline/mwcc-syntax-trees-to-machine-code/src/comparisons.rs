//! Branchless comparison idioms.

use crate::analysis::*;
use crate::expressions::load_base_name;
use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    /// Materialize `unsigned narrow < C` as the sign bit of `value - C`.
    /// A zero-extended byte/halfword never reaches bit 31, so subtraction by a
    /// positive 16-bit immediate makes that bit exactly the borrow predicate:
    /// `lhz d; addi r0,d,-C; srwi d,r0,31`.
    pub(crate) fn try_emit_unsigned_narrow_less_constant(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Less || self.signedness_of(left)? {
            return Ok(false);
        }
        let is_unsigned_narrow = match left {
            Expression::Variable(name) => self.locations.get(name).is_some_and(|location| {
                location.class == ValueClass::General && location.width <= 16 && !location.signed
            }),
            Expression::Member { member_type, .. } => {
                matches!(member_type, mwcc_syntax_trees::Type::UnsignedChar | mwcc_syntax_trees::Type::UnsignedShort)
            }
            _ => false,
        };
        if !is_unsigned_narrow {
            return Ok(false);
        }
        let Some(negative_constant) = constant_value(right)
            .filter(|constant| *constant > 0)
            .and_then(|constant| constant.checked_neg())
            .and_then(|constant| i16::try_from(constant).ok())
        else {
            return Ok(false);
        };

        self.evaluate_general(left, destination)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: destination,
            immediate: negative_constant,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: destination,
                s: GENERAL_SCRATCH,
                shift: 31,
            });
        Ok(true)
    }

    /// Emit a comparison as mwcc's branchless idiom. Currently handles `==` (and
    /// `== 0`) and signed `< 0`; the richer signed less/greater idioms are not
    /// implemented yet.
    /// A register holding `value`, distinct from GENERAL_SCRATCH, for the branchless
    /// sign idioms that negate/complement into the scratch and then OR/AND with the
    /// value: a full-width leaf stays in its home register (`neg r0,r3; ... r0,r0,r3`).
    /// A sub-expression evaluates into the destination — unless that *is* the scratch
    /// (a store, d=r0), where the scratch op would clobber it, which defers.
    fn sign_idiom_source(&mut self, value: &Expression, destination: u8) -> Compilation<u8> {
        if let Some(register) = self
            .leaf_info(value)
            .ok()
            .filter(|&(_, width, _)| width == 32)
            .map(|(register, _, _)| register)
        {
            return Ok(register);
        }
        // A signed byte load is brought into the scratch and sign-extended into the destination
        // (`lbz r0; extsb d,r0`), matching mwcc's `> 0` / `!= 0` register choice.
        if destination != GENERAL_SCRATCH && self.is_signed_byte_load(value)? {
            self.evaluate_general(value, GENERAL_SCRATCH)?;
            self.emit_widen(destination, GENERAL_SCRATCH, 8, true);
            return Ok(destination);
        }
        if destination != GENERAL_SCRATCH {
            self.evaluate_general(value, destination)?;
            Ok(destination)
        } else {
            Err(Diagnostic::error("a branchless comparison of a narrow or non-leaf value into the scratch needs a second register (roadmap)"))
        }
    }

    pub(crate) fn emit_comparison(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        // A comparison whose operands are floating-point materializes a boolean
        // from cr0 rather than using the integer branchless idioms below.
        if self.is_float_leaf(left) || self.is_float_leaf(right) {
            return self.emit_float_comparison(operator, left, right, destination);
        }
        // An INTEGER comparison of a value against itself (`x == x`, `a[0] < a[0]`)
        // is a compile-time constant — mwcc folds it to `li 0`/`li 1` without
        // evaluating the operand. (Floats are excluded above: `NaN == NaN` is
        // false, so that fold would be wrong.)
        if same_operand(left, right) {
            let value = i64::from(matches!(
                operator,
                BinaryOperator::Equal | BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
            ));
            self.load_integer_constant(destination, value);
            return Ok(());
        }
        let d = destination;
        // A RELATIONAL comparison (`< <= > >=`) is UNSIGNED if EITHER operand is unsigned
        // (C's usual arithmetic conversions): `int a < unsigned b` uses mwcc's unsigned idiom,
        // not the signed `srawi` form — signed only when both operands are signed. `==`/`!=`
        // are signedness-INDEPENDENT (the bit patterns differ iff the values do), and mwcc
        // keys their idiom off the left operand's declared type, so they take its signedness
        // alone. (The `x OP 0` idioms test the left's sign; there the literal 0 is signed, so
        // the relational form equals the left's signedness too.)
        let signed_comparison =
            if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                self.signedness_of(left)?
            } else {
                self.signedness_of(left)? && self.signedness_of(right)?
            };
        if self.try_emit_unsigned_narrow_less_constant(operator, left, right, d)? {
            return Ok(());
        }
        // Unsigned comparisons against literal ZERO collapse to `== 0` / `!= 0` — since an
        // unsigned value is always >= 0, `u > 0` (and `0 < u`) is `u != 0`, and `u <= 0` (and
        // `0 >= u`) is `u == 0`. mwcc emits the cheaper equality idiom for these. (It keeps
        // `u >= 1` / `u < 1` as their own relational idioms, so those are NOT folded; and signed
        // comparisons are unaffected — `int a > 0` is not `a != 0`.)
        if !signed_comparison {
            // The remaining relations at the unsigned domain boundary are constants:
            // `u < 0` / `0 > u` can never hold, while `u >= 0` / `0 <= u` always do.
            // mwcc removes the operand evaluation entirely for these non-volatile shapes.
            let constant = match operator {
                BinaryOperator::Less if is_zero_literal(right) => Some(0),
                BinaryOperator::GreaterEqual if is_zero_literal(right) => Some(1),
                BinaryOperator::Greater if is_zero_literal(left) => Some(0),
                BinaryOperator::LessEqual if is_zero_literal(left) => Some(1),
                _ => None,
            };
            if let Some(value) = constant {
                self.load_integer_constant(d, value);
                return Ok(());
            }
            let folded = match operator {
                BinaryOperator::Greater if is_zero_literal(right) => {
                    Some((BinaryOperator::NotEqual, left))
                }
                BinaryOperator::LessEqual if is_zero_literal(right) => {
                    Some((BinaryOperator::Equal, left))
                }
                BinaryOperator::Less if is_zero_literal(left) => {
                    Some((BinaryOperator::NotEqual, right))
                }
                BinaryOperator::GreaterEqual if is_zero_literal(left) => {
                    Some((BinaryOperator::Equal, right))
                }
                _ => None,
            };
            if let Some((equality_operator, operand)) = folded {
                let zero = Expression::IntegerLiteral(0);
                return self.emit_comparison(equality_operator, operand, &zero, d);
            }
        }
        // A comparison is already canonical 0/1. Testing it `!= 0` is an
        // identity and must be folded before choosing a build-specific value
        // idiom; otherwise the legacy selector booleanizes the inner result a
        // second time (`(a < b) != 0`).
        if operator == BinaryOperator::NotEqual && is_zero_literal(right) {
            if let Expression::Binary {
                operator: inner_operator,
                left: inner_left,
                right: inner_right,
            } = left
            {
                if is_comparison(*inner_operator) {
                    return self.emit_comparison(*inner_operator, inner_left, inner_right, d);
                }
            }
        }
        if self.try_emit_legacy_integer_comparison(operator, left, right, d, signed_comparison)? {
            return Ok(());
        }
        match operator {
            BinaryOperator::Equal => {
                if is_zero_literal(right) || is_zero_literal(left) {
                    let value = if is_zero_literal(right) { left } else { right };
                    // `(comparison) == 0` is the NEGATED comparison — `(a < b) == 0` is `a >= b`
                    // (one idiom), not "compute `a < b` to 0/1, then test that against 0". mwcc
                    // folds the double-test; `!(a < b)` already did, so match it here too.
                    if let Expression::Binary {
                        operator: inner_operator,
                        left: inner_left,
                        right: inner_right,
                    } = value
                    {
                        if let Some(flipped) = flip_comparison(*inner_operator) {
                            return self.emit_comparison(flipped, inner_left, inner_right, d);
                        }
                    }
                    // `(x & (1<<k)) == 0`: extract bit k to the low bit (one rlwinm),
                    // then flip it. Build 163 instead preserves the masked value
                    // and feeds it through its negate/cntlzw equality sequence.
                    if !self.behavior.negate_before_zero_equality {
                        if let Some((variable, mask)) = as_masked_leaf(value) {
                            if mask.is_power_of_two() {
                                let register = self.general_register_of_leaf(variable)?;
                                let shift = ((32 - mask.trailing_zeros()) % 32) as u8;
                                self.output.instructions.push(Instruction::RotateAndMask {
                                    a: GENERAL_SCRATCH,
                                    s: register,
                                    shift,
                                    begin: 31,
                                    end: 31,
                                });
                                self.output.instructions.push(Instruction::XorImmediate {
                                    a: d,
                                    s: GENERAL_SCRATCH,
                                    immediate: 1,
                                });
                                return Ok(());
                            }
                        }
                    }
                    // A signed byte load is `lbz` (zero-extended); mwcc loads it into the scratch
                    // and re-extends in place (`lbz r0; extsb r0,r0`) before the leading-zero test,
                    // keeping the value in the scratch — going through place_operand would extend
                    // into the destination and double-extend. Signed halfword loads use `lha`
                    // (already sign-extended) and unsigned loads need nothing.
                    let source = if self.is_signed_byte_load(value)? {
                        self.evaluate_general(value, GENERAL_SCRATCH)?;
                        self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                        GENERAL_SCRATCH
                    } else {
                        self.place_operand_or_scratch(value, d)?
                    };
                    let source = if self.behavior.negate_before_zero_equality {
                        self.output.instructions.push(Instruction::Negate {
                            d: GENERAL_SCRATCH,
                            a: source,
                        });
                        GENERAL_SCRATCH
                    } else {
                        source
                    };
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: source,
                        });
                } else if let Some(constant) = as_small_integer(right) {
                    // a == c : (c - a) leading zeros. A signed byte load comes into the scratch and
                    // is sign-extended in place (`lbz r0; extsb r0,r0`); a narrow leaf operand is
                    // extended into the scratch (extsb/clrlwi); a full-word load is evaluated into
                    // the scratch; a wide leaf stays in its register.
                    let value = if self.is_signed_byte_load(left)? {
                        self.evaluate_general(left, GENERAL_SCRATCH)?;
                        self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                        GENERAL_SCRATCH
                    } else if self.is_byte_load(left) {
                        // `lbz` is already the promoted unsigned-byte value. Compare it
                        // directly in the scratch; adding `clrlwi` would be redundant.
                        self.evaluate_general(left, GENERAL_SCRATCH)?;
                        GENERAL_SCRATCH
                    } else if self.is_word_load(left) {
                        self.evaluate_general(left, GENERAL_SCRATCH)?;
                        GENERAL_SCRATCH
                    } else {
                        match self.leaf_info(left) {
                            Ok((register, width, signed)) if width < 32 => {
                                self.emit_widen(GENERAL_SCRATCH, register, width, signed);
                                GENERAL_SCRATCH
                            }
                            _ => self.general_register_of_leaf(left)?,
                        }
                    };
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromImmediate {
                            d: GENERAL_SCRATCH,
                            a: value,
                            immediate: constant,
                        });
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                        });
                } else {
                    // a == b : leading zeros of (a - b). Narrow operands are
                    // extended first — the left in place, the right into the
                    // scratch (mwcc's placement for the equality idiom).
                    let (left_register, right_register) = self.place_compare_leaves(left, right)?;
                    self.output.instructions.push(Instruction::SubtractFrom {
                        d: GENERAL_SCRATCH,
                        a: left_register,
                        b: right_register,
                    });
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                        });
                }
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 5,
                    });
                Ok(())
            }
            // x != 0 : sign bit of (-x | x)
            BinaryOperator::NotEqual if is_zero_literal(right) => {
                // `(x & (1<<k)) != 0`: extract bit k to the low bit with one rlwinm.
                if let Some((variable, mask)) = as_masked_leaf(left) {
                    if mask.is_power_of_two() {
                        let register = self.general_register_of_leaf(variable)?;
                        let shift = ((32 - mask.trailing_zeros()) % 32) as u8;
                        self.output.instructions.push(Instruction::RotateAndMask {
                            a: d,
                            s: register,
                            shift,
                            begin: 31,
                            end: 31,
                        });
                        return Ok(());
                    }
                }
                // `(-x | x) >> 31`: the top bit is set iff x has any bit set. A signed-byte load
                // (`lbz`, zero-extended) is first sign-extended like mwcc — `lbz` into the scratch,
                // `extsb` into d — then the idiom runs on d. (The `== 0` leading-zero case above
                // extends the same way; the truthiness path previously skipped it.)
                let source = if self.is_signed_byte_load(left)? && d != GENERAL_SCRATCH {
                    self.evaluate_general(left, GENERAL_SCRATCH)?;
                    self.emit_widen(d, GENERAL_SCRATCH, 8, true);
                    d
                } else {
                    self.sign_idiom_source(left, d)?
                };
                self.output.instructions.push(Instruction::Negate {
                    d: GENERAL_SCRATCH,
                    a: source,
                });
                self.output.instructions.push(Instruction::Or {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    b: source,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                Ok(())
            }
            // `x != C` (nonzero constant): sign bit of ((C - x) | (x - C)), both
            // halves built with immediates (`subfic` and `addi`).
            BinaryOperator::NotEqual
                if leaf_name(left).is_some()
                    && !self.is_narrow_leaf(left)
                    && constant_value(right).is_some_and(|constant| {
                        constant != 0
                            && i16::try_from(constant).is_ok()
                            && i16::try_from(-constant).is_ok()
                    }) =>
            {
                let constant = constant_value(right).unwrap() as i16;
                let x = self.general_register_of_leaf(left)?;
                let Some(temp) =
                    (3u8..=12).find(|register| *register != x && !self.reserved.contains(register))
                else {
                    return Err(Diagnostic::error("out of registers for the != idiom"));
                };
                self.output
                    .instructions
                    .push(Instruction::SubtractFromImmediate {
                        d: temp,
                        a: x,
                        immediate: constant,
                    });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: x,
                    immediate: -constant,
                });
                self.output.instructions.push(Instruction::Or {
                    a: GENERAL_SCRATCH,
                    s: temp,
                    b: GENERAL_SCRATCH,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                Ok(())
            }
            // signed x < 0 : the sign bit. A signed-char load is brought into the scratch and
            // sign-extended in place (`lbz r0; extsb r0,r0`), matching mwcc, rather than through
            // place_operand (which sign-extends into the destination and would mismatch the idiom's
            // register).
            BinaryOperator::Less if is_zero_literal(right) && signed_comparison => {
                let source = if self.is_signed_byte_load(left)? {
                    self.evaluate_general(left, GENERAL_SCRATCH)?;
                    self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                    GENERAL_SCRATCH
                } else {
                    self.place_operand_or_scratch(left, d)?
                };
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: source,
                        shift: 31,
                    });
                Ok(())
            }
            // signed x > 0 : sign bit of (-x & ~x)
            BinaryOperator::Greater if is_zero_literal(right) && signed_comparison => {
                let source = self.sign_idiom_source(left, d)?;
                self.output.instructions.push(Instruction::Negate {
                    d: GENERAL_SCRATCH,
                    a: source,
                });
                self.output.instructions.push(Instruction::AndComplement {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    b: source,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                Ok(())
            }
            // signed x >= 0 : !(x < 0). A signed-char load comes into the scratch, extended in
            // place (`lbz r0; extsb r0,r0; srwi r0,r0,31; xori r3,r0,1`).
            BinaryOperator::GreaterEqual if is_zero_literal(right) && signed_comparison => {
                let source = if self.is_signed_byte_load(left)? {
                    self.evaluate_general(left, GENERAL_SCRATCH)?;
                    self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                    GENERAL_SCRATCH
                } else {
                    self.place_operand_or_scratch(left, d)?
                };
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: GENERAL_SCRATCH,
                        s: source,
                        shift: 31,
                    });
                self.output.instructions.push(Instruction::XorImmediate {
                    a: d,
                    s: GENERAL_SCRATCH,
                    immediate: 1,
                });
                Ok(())
            }
            // signed x <= 0 : `cntlzw(x)` is 0 (x<0) or 32 (x==0) but 1..31 (x>0), so
            // rotating a `1` left by that count lands in the low bit only for x <= 0.
            // A full-width leaf reads x with cntlzw into the scratch, then puts the `1`
            // in x's now-free home register and rotates (`cntlzw r0,r3; li r3,1;
            // rlwnm r0,r3,r0`) — putting the `1` in the destination would let the cntlzw
            // clobber it for a store (d=r0). A non-leaf keeps the original scratch path.
            BinaryOperator::LessEqual if is_zero_literal(right) && signed_comparison => {
                if let Some(register) = self
                    .leaf_info(left)
                    .ok()
                    .filter(|&(_, width, _)| width == 32)
                    .map(|(register, _, _)| register)
                {
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: register,
                        });
                    self.load_integer_constant(register, 1);
                    self.output
                        .instructions
                        .push(Instruction::RotateAndMaskVariable {
                            a: d,
                            s: register,
                            b: GENERAL_SCRATCH,
                            begin: 31,
                            end: 31,
                        });
                    return Ok(());
                }
                // A signed-char load keeps the value in the scratch and sign-extends in place,
                // putting the `1` in the destination between the load and the extend:
                // `lbz r0; li r3,1; extsb r0,r0; cntlzw r0,r0; rlwnm r3,r3,r0`.
                if self.is_signed_byte_load(left)? && d != GENERAL_SCRATCH {
                    self.evaluate_general(left, GENERAL_SCRATCH)?;
                    self.load_integer_constant(d, 1);
                    self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                        });
                    self.output
                        .instructions
                        .push(Instruction::RotateAndMaskVariable {
                            a: d,
                            s: d,
                            b: GENERAL_SCRATCH,
                            begin: 31,
                            end: 31,
                        });
                    return Ok(());
                }
                let source = self.place_operand_or_scratch(left, d)?;
                if source == d {
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: source,
                        });
                    self.load_integer_constant(d, 1);
                } else {
                    self.load_integer_constant(d, 1);
                    self.output
                        .instructions
                        .push(Instruction::CountLeadingZeros {
                            a: GENERAL_SCRATCH,
                            s: source,
                        });
                }
                self.output
                    .instructions
                    .push(Instruction::RotateAndMaskVariable {
                        a: d,
                        s: d,
                        b: GENERAL_SCRATCH,
                        begin: 31,
                        end: 31,
                    });
                Ok(())
            }
            // general signed branchless comparisons. Both leaves (any operator), or
            // a one-non-leaf shape whose idiom uses that operand twice, so mwcc keeps
            // it in a register: the `>` idiom keeps its LEFT operand, the `<` idiom
            // keeps its RIGHT. Computing that operand into a virtual that *avoids the
            // destination* leaves the destination free for the idiom's result-path
            // temporary — reproducing mwcc (p->a > x, (a+b) > c; x < p->a, x < (a+b)).
            // The other idioms use an operand once (mwcc keeps it in the scratch);
            // those non-leaf shapes still defer rather than mismatch.
            // signed `x > C` : materialize C in r0 (the `>` idiom uses the right
            // operand once), then the same sign-bit idiom with a fresh temp. `x` is
            // a leaf or a full-word load (e.g. `*p > C`, `s->a > C`).
            BinaryOperator::Greater
                if signed_comparison
                    && !self.is_narrow_leaf(left)
                    && (leaf_name(left).is_some() || self.is_simple_word_load(left))
                    && constant_value(right)
                        .is_some_and(|constant| i16::try_from(constant).is_ok()) =>
            {
                let x = if leaf_name(left).is_some() {
                    self.general_register_of_leaf(left)?
                } else {
                    // The load avoids the destination so the later temp (the
                    // intermediate `(x^C)>>1`) can coalesce onto it, as mwcc does.
                    let register = self.fresh_virtual_general_avoiding(vec![d]);
                    self.evaluate_general(left, register)?;
                    register
                };
                let constant = constant_value(right).unwrap();
                self.load_integer_constant(GENERAL_SCRATCH, constant);
                let scratch = GENERAL_SCRATCH;
                let temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::Xor {
                    a: scratch,
                    s: x,
                    b: scratch,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: temp,
                        s: scratch,
                        shift: 1,
                    });
                self.output.instructions.push(Instruction::And {
                    a: scratch,
                    s: scratch,
                    b: x,
                });
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: scratch,
                    a: scratch,
                    b: temp,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: scratch,
                        shift: 31,
                    });
                Ok(())
            }
            // signed `(load) < C` : the load is the low operand (read once) → r0;
            // the constant is the high operand (read twice) → a fresh register.
            BinaryOperator::Less
                if signed_comparison
                    && self.is_simple_word_load(left)
                    && !self.is_narrow_leaf(right)
                    && constant_value(right)
                        .is_some_and(|constant| i16::try_from(constant).is_ok()) =>
            {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                let load = GENERAL_SCRATCH;
                let constant_register = self.fresh_virtual_general();
                self.load_integer_constant(constant_register, constant_value(right).unwrap());
                let scratch = GENERAL_SCRATCH;
                self.output.instructions.push(Instruction::Xor {
                    a: scratch,
                    s: constant_register,
                    b: load,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: d,
                        s: scratch,
                        shift: 1,
                    });
                self.output.instructions.push(Instruction::And {
                    a: scratch,
                    s: scratch,
                    b: constant_register,
                });
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: scratch,
                    a: scratch,
                    b: d,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: scratch,
                        shift: 31,
                    });
                Ok(())
            }
            // `a[i] != a[j]` / `*p != *q` (two full-word loads): the left operand
            // loads into a fresh virtual, the right into the scratch, in source
            // order, then the leaf != idiom — sign bit of ((b-a)|(a-b)). Equality
            // is sign-agnostic so this also covers unsigned loads. The left value
            // is live across both subtracts, and mwcc keeps it off BOTH base
            // registers (a same base stays live for the second load; a different
            // base is dead but mwcc still does not reuse it) — so the left virtual
            // avoids both bases: it colors r4 for a shared base, r5 for distinct
            // bases, matching `lwz r4/r5; lwz r0; subf …`.
            BinaryOperator::NotEqual
                if self.is_simple_word_load(left)
                    && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(right).is_some() =>
            {
                let avoid: Vec<u8> = [&*left, &*right]
                    .iter()
                    .filter_map(|operand| {
                        load_base_name(operand).and_then(|name| self.lookup_general(name))
                    })
                    .collect();
                let left_register = self.fresh_virtual_general_avoiding(avoid);
                self.evaluate_general(left, left_register)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                let right_register = GENERAL_SCRATCH;
                let scratch = GENERAL_SCRATCH;
                let temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: temp,
                    a: left_register,
                    b: right_register,
                });
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: scratch,
                    a: right_register,
                    b: left_register,
                });
                self.output.instructions.push(Instruction::Or {
                    a: scratch,
                    s: temp,
                    b: scratch,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: scratch,
                        shift: 31,
                    });
                Ok(())
            }
            // `a[i] < a[j]` / `a[i] > a[j]` (two same-base full-word loads): the
            // operand the signed idiom reads twice goes to a fresh virtual (the
            // allocator colors it r4, the base reg being live), the once-read
            // operand to the scratch; loads in source order. `<` reads its RIGHT
            // operand twice (`xor`+`and`), `>` reads its LEFT twice. Different
            // bases defer (the same allocator-coloring gap as the != case).
            //
            // The `>` form loads its twice-read operand FIRST, while the shared
            // base is still needed for the second load, so that operand is forced
            // off the (soon-dead) base register exactly as mwcc places it — it
            // matches in every destination context. The `<` form loads its
            // twice-read operand SECOND, after the base dies; there it relies on
            // the fixed `srawi → d` write to keep that operand off the destination
            // register, which only reproduces mwcc when `d` is a real register
            // (return/arg). In a value/store context (`d` == scratch) that write
            // would collide with the scratch and mwcc declines to reuse the dead
            // base anyway, so `<` defers there.
            BinaryOperator::Less | BinaryOperator::Greater
                if signed_comparison
                    && self.is_simple_word_load(left)
                    && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(right).is_some()
                    && (matches!(operator, BinaryOperator::Greater) || d != GENERAL_SCRATCH) =>
            {
                let scratch = GENERAL_SCRATCH;
                let left_base = load_base_name(left).and_then(|name| self.lookup_general(name));
                let right_base = load_base_name(right).and_then(|name| self.lookup_general(name));
                if matches!(operator, BinaryOperator::Less) {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b)). b is read twice
                    // and loaded second, so it reuses its own (now-dead) base — it
                    // only avoids the LEFT operand's base (r4 same-base, or the
                    // right base it coalesces onto for distinct bases).
                    self.evaluate_general(left, scratch)?;
                    let right_register =
                        self.fresh_virtual_general_avoiding(left_base.into_iter().collect());
                    self.evaluate_general(right, right_register)?;
                    self.output.instructions.push(Instruction::Xor {
                        a: scratch,
                        s: right_register,
                        b: scratch,
                    });
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightAlgebraicImmediate {
                            a: d,
                            s: scratch,
                            shift: 1,
                        });
                    self.output.instructions.push(Instruction::And {
                        a: scratch,
                        s: scratch,
                        b: right_register,
                    });
                    self.output.instructions.push(Instruction::SubtractFrom {
                        d: scratch,
                        a: scratch,
                        b: d,
                    });
                } else {
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)). a is read twice
                    // and loaded first, so it must stay off BOTH bases (its own is
                    // live during the load, the other until the second load) — r4
                    // same-base, r5 distinct.
                    let left_register = self.fresh_virtual_general_avoiding(
                        [left_base, right_base].into_iter().flatten().collect(),
                    );
                    self.evaluate_general(left, left_register)?;
                    self.evaluate_general(right, scratch)?;
                    let temp = self.fresh_virtual_general();
                    self.output.instructions.push(Instruction::Xor {
                        a: scratch,
                        s: left_register,
                        b: scratch,
                    });
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightAlgebraicImmediate {
                            a: temp,
                            s: scratch,
                            shift: 1,
                        });
                    self.output.instructions.push(Instruction::And {
                        a: scratch,
                        s: scratch,
                        b: left_register,
                    });
                    self.output.instructions.push(Instruction::SubtractFrom {
                        d: scratch,
                        a: scratch,
                        b: temp,
                    });
                }
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: scratch,
                        shift: 31,
                    });
                Ok(())
            }
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::NotEqual
                if signed_comparison
                    && !self.is_narrow_leaf(left)
                    && !self.is_narrow_leaf(right)
                    && ((leaf_name(left).is_some() && leaf_name(right).is_some())
                        || (matches!(operator, BinaryOperator::Greater)
                            && leaf_name(left).is_none()
                            && leaf_name(right).is_some())
                        || (matches!(operator, BinaryOperator::Less)
                            && leaf_name(right).is_none()
                            && leaf_name(left).is_some())
                        || (matches!(operator, BinaryOperator::NotEqual)
                            && leaf_name(left).is_none() != leaf_name(right).is_none())) =>
            {
                let (left_register, right_register) =
                    self.place_compare_operands(operator, left, right, d)?;
                let scratch = GENERAL_SCRATCH;
                match operator {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b)). Like `>`, the
                    // intermediate `(a^b)>>1` goes to a fresh virtual (the allocator
                    // coalesces it onto rA, free after the xor — mwcc's `srawi r3`), not
                    // the destination: writing it into d would clobber the xor result in
                    // the scratch when d *is* the scratch (a value/store, d=r0).
                    BinaryOperator::Less => {
                        let temp = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::Xor {
                            a: scratch,
                            s: right_register,
                            b: left_register,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightAlgebraicImmediate {
                                a: temp,
                                s: scratch,
                                shift: 1,
                            });
                        self.output.instructions.push(Instruction::And {
                            a: scratch,
                            s: scratch,
                            b: right_register,
                        });
                        self.output.instructions.push(Instruction::SubtractFrom {
                            d: scratch,
                            a: scratch,
                            b: temp,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightLogicalImmediate {
                                a: d,
                                s: scratch,
                                shift: 31,
                            });
                    }
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)). The intermediate
                    // `(a^b)>>1` goes to a fresh virtual the allocator places at the
                    // lowest free register — for leaves that coalesces onto rB (free
                    // after the xor), reproducing mwcc, and it stays correct when an
                    // operand is a load and rB is not free.
                    BinaryOperator::Greater => {
                        let temp = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::Xor {
                            a: scratch,
                            s: left_register,
                            b: right_register,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightAlgebraicImmediate {
                                a: temp,
                                s: scratch,
                                shift: 1,
                            });
                        self.output.instructions.push(Instruction::And {
                            a: scratch,
                            s: scratch,
                            b: left_register,
                        });
                        self.output.instructions.push(Instruction::SubtractFrom {
                            d: scratch,
                            a: scratch,
                            b: temp,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightLogicalImmediate {
                                a: d,
                                s: scratch,
                                shift: 31,
                            });
                    }
                    // a != b : sign bit of ((b - a) | (a - b)), with a second temp.
                    _ => {
                        let temp = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::SubtractFrom {
                            d: temp,
                            a: left_register,
                            b: right_register,
                        });
                        self.output.instructions.push(Instruction::SubtractFrom {
                            d: scratch,
                            a: right_register,
                            b: left_register,
                        });
                        self.output.instructions.push(Instruction::Or {
                            a: scratch,
                            s: temp,
                            b: scratch,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightLogicalImmediate {
                                a: d,
                                s: scratch,
                                shift: 31,
                            });
                    }
                }
                Ok(())
            }
            // unsigned a < b / a > b : xor/cntlzw/slw/srwi. A constant operand `x > C`
            // is the low side (read once) → r0; `x < C` is the high side (read twice)
            // → a fresh register the allocator places at the lowest free GPR.
            BinaryOperator::Less | BinaryOperator::Greater
                if !signed_comparison
                    && leaf_name(left).is_some()
                    && !self.is_narrow_leaf(left)
                    && !self.is_narrow_leaf(right)
                    && (leaf_name(right).is_some()
                        || constant_value(right)
                            .is_some_and(|constant| i16::try_from(constant).is_ok())) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = match constant_value(right) {
                    Some(constant) if matches!(operator, BinaryOperator::Less) => {
                        let register = self.fresh_virtual_general();
                        self.load_integer_constant(register, constant);
                        register
                    }
                    _ => self.compare_right_operand(right)?,
                };
                // a < b uses b as the high side; a > b is b < a.
                let high = if matches!(operator, BinaryOperator::Less) {
                    right_register
                } else {
                    left_register
                };
                let low = if matches!(operator, BinaryOperator::Less) {
                    left_register
                } else {
                    right_register
                };
                self.output.instructions.push(Instruction::Xor {
                    a: GENERAL_SCRATCH,
                    s: high,
                    b: low,
                });
                self.output
                    .instructions
                    .push(Instruction::CountLeadingZeros {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                    });
                self.output.instructions.push(Instruction::ShiftLeftWord {
                    a: GENERAL_SCRATCH,
                    s: high,
                    b: GENERAL_SCRATCH,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                Ok(())
            }
            // unsigned a <= b / a >= b : orc-based, dest + scratch.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if !signed_comparison
                    && leaf_name(left).is_some()
                    && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left)
                    && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a<=b uses (low,high)=(a,b); a>=b is b<=a.
                let (low, high) = match operator {
                    BinaryOperator::LessEqual => (left_register, right_register),
                    _ => (right_register, left_register),
                };
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: GENERAL_SCRATCH,
                    a: low,
                    b: high,
                });
                self.output.instructions.push(Instruction::OrComplement {
                    a: d,
                    s: high,
                    b: low,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        shift: 1,
                    });
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: GENERAL_SCRATCH,
                    a: GENERAL_SCRATCH,
                    b: d,
                });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                Ok(())
            }
            // `a[i] <= a[j]` / `s->x >= s->y` (two same-base full-word loads): the
            // carry idiom over loaded operands. The operands load high-first — one
            // into the scratch, the other into a free register; sign(high) goes to
            // another free register and sign(low) to the destination:
            // `lwz r0; lwz r5; srawi r4,high,31; srwi d,low,31; subfc r0,low,high;
            // adde d,r4,d`. Different bases / value context defer.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_comparison
                    && self.is_simple_word_load(left)
                    && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(left) == load_base_name(right)
                    && d != GENERAL_SCRATCH =>
            {
                let base = load_base_name(left).and_then(|name| self.lookup_general(name));
                let mut free = (3u8..=12).filter(|r| {
                    *r != GENERAL_SCRATCH
                        && *r != d
                        && Some(*r) != base
                        && !self.reserved.contains(r)
                });
                let (Some(sign_high_reg), Some(operand_reg)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error(
                        "out of registers for the two-load <=/>= idiom",
                    ));
                };
                let scratch = GENERAL_SCRATCH;
                let (high, low) = if matches!(operator, BinaryOperator::LessEqual) {
                    (right, left)
                } else {
                    (left, right)
                };
                // The high operand loads first; for `<=` it lands in the scratch,
                // for `>=` in the free register (and the low operand vice versa).
                let (high_reg, low_reg) = if matches!(operator, BinaryOperator::LessEqual) {
                    self.evaluate_general(high, scratch)?;
                    self.evaluate_general(low, operand_reg)?;
                    (scratch, operand_reg)
                } else {
                    self.evaluate_general(high, operand_reg)?;
                    self.evaluate_general(low, scratch)?;
                    (operand_reg, scratch)
                };
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: sign_high_reg,
                        s: high_reg,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: low_reg,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: scratch,
                        a: low_reg,
                        b: high_reg,
                    });
                self.output.instructions.push(Instruction::AddExtended {
                    d,
                    a: sign_high_reg,
                    b: d,
                });
                Ok(())
            }
            // signed a <= b / a >= b : carry-based, with two temporaries. A
            // constant right operand materializes into r0 (read twice before being
            // overwritten by the subfc).
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_comparison
                    && leaf_name(left).is_some()
                    && !self.is_narrow_leaf(left)
                    && !self.is_narrow_leaf(right)
                    && (leaf_name(right).is_some()
                        || constant_value(right)
                            .is_some_and(|constant| i16::try_from(constant).is_ok())) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.compare_right_operand(right)?;
                let mut free = (3u8..=12)
                    .filter(|r| ![left_register, right_register, GENERAL_SCRATCH].contains(r));
                let (Some(lower), Some(higher)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error("out of registers for comparison"));
                };
                // For a<=b: high = sign(b), low = sign(a), carry from (b - a).
                // For a>=b the operands swap.
                let (sign_high, sign_low, subtrahend, minuend) = match operator {
                    BinaryOperator::LessEqual => {
                        (right_register, left_register, left_register, right_register)
                    }
                    _ => (left_register, right_register, right_register, left_register),
                };
                let sign_of_high = Instruction::ShiftRightAlgebraicImmediate {
                    a: higher,
                    s: sign_high,
                    shift: 31,
                };
                let sign_of_low = Instruction::ShiftRightLogicalImmediate {
                    a: lower,
                    s: sign_low,
                    shift: 31,
                };
                // With a materialized constant, mwcc shifts the ready variable
                // operand (the left leaf) before the constant's.
                if constant_value(right).is_some() && sign_low == left_register {
                    self.output.instructions.push(sign_of_low);
                    self.output.instructions.push(sign_of_high);
                } else {
                    self.output.instructions.push(sign_of_high);
                    self.output.instructions.push(sign_of_low);
                }
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: GENERAL_SCRATCH,
                        a: subtrahend,
                        b: minuend,
                    });
                self.output.instructions.push(Instruction::AddExtended {
                    d,
                    a: higher,
                    b: lower,
                });
                Ok(())
            }
            _ => Err(Diagnostic::error(
                "this comparison needs the branchless compare idioms (roadmap)",
            )),
        }
    }

    /// A comparison whose operands are floating-point. mwcc compares into cr0
    /// (`fcmpu` for `==`/`!=`, `fcmpo` for the ordered relations), then moves cr0
    /// into a GPR with `mfcr` and rotates the relevant bit (lt=0, gt=1, eq=2) down
    /// to the low bit. `<=`/`>=` first fold equality into the eq bit with `cror`;
    /// `!=` extracts eq and flips it with `xori`.
    pub(crate) fn emit_float_comparison(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        const LT: u8 = 0;
        const GT: u8 = 1;
        const EQ: u8 = 2;
        // The comparison's precision comes from the typed (non-literal) operand; a
        // float literal (e.g. `x > 0.0`) is loaded from the constant pool.
        let double = self.is_double_value(left) || self.is_double_value(right);
        let a = self.place_float_compare_operand(left, double)?;
        let b = self.place_float_compare_operand(right, double)?;
        // mfcr writes the final destination directly. The following rotate is
        // destructive and consumes no prior destination value, so routing it
        // through r0 would add an unnecessary register edge and differs from
        // mwcc's `mfcr r3; rlwinm r3,r3,...` return schedule.
        let scratch = destination;
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            // `==`/`!=` are commutative; mwcc canonicalizes a literal operand to
            // the front (it loaded the constant first), so `x == 0.0` is `fcmpu 0,x`.
            let (first, second) = if matches!(
                right,
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            ) {
                (b, a)
            } else {
                (a, b)
            };
            self.output
                .instructions
                .push(Instruction::FloatCompareUnordered {
                    a: first,
                    b: second,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::FloatCompareOrdered { a, b });
        }
        // `<=`/`>=` fold equality into the eq bit so one extract covers both relations.
        match operator {
            BinaryOperator::LessEqual => {
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterOr {
                        d: EQ,
                        a: LT,
                        b: EQ,
                    })
            }
            BinaryOperator::GreaterEqual => {
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterOr {
                        d: EQ,
                        a: GT,
                        b: EQ,
                    })
            }
            _ => {}
        }
        self.output
            .instructions
            .push(Instruction::MoveFromConditionRegister { d: scratch });
        let bit = match operator {
            BinaryOperator::Less => LT,
            BinaryOperator::Greater => GT,
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual => EQ,
            _ => return Err(Diagnostic::error("unsupported floating-point comparison")),
        };
        // Rotate the bit (at position `bit` from the MSB) into bit 31 and mask it.
        let shift = bit + 1;
        if matches!(operator, BinaryOperator::NotEqual) {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: scratch,
                s: scratch,
                shift,
                begin: 31,
                end: 31,
            });
            self.output.instructions.push(Instruction::XorImmediate {
                a: destination,
                s: scratch,
                immediate: 1,
            });
        } else {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: destination,
                s: scratch,
                shift,
                begin: 31,
                end: 31,
            });
        }
        Ok(())
    }

    /// A floating-point comparison used as a *condition* (in an `if`): emit the
    /// `fcmpo`/`fcmpu` (and the `cror` that folds equality into the eq bit for
    /// `<=`/`>=`) and return the branch `(options, bit)` that skips the guarded body
    /// when the relation is false — the same bit mapping the integer compare uses.
    pub(crate) fn emit_float_condition(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<(u8, u8)> {
        const LT: u8 = 0;
        const GT: u8 = 1;
        const EQ: u8 = 2;
        const FLOAT_FIRST: u8 = 1; // f1
        let double = self.is_double_value(left) || self.is_double_value(right);
        let eq = matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual);
        let left_literal = matches!(
            left,
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        );
        let right_literal = matches!(
            right,
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        );
        let left_load = self.is_float_operand(left) && !self.is_float_leaf(left) && !left_literal;
        let right_load =
            self.is_float_operand(right) && !self.is_float_leaf(right) && !right_literal;
        let mut left_is_float = self.is_float_operand(left);
        let mut right_is_float = self.is_float_operand(right);
        // An unsuffixed integer constant in a comparison with a floating value
        // undergoes the usual arithmetic conversion. Keep nonconstant integer
        // operands on the measured magic-bias paths below, but let pool-literal
        // placement handle `float_value < 0` and its mirrored spelling.
        if left_is_float
            && matches!(
                right,
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            )
        {
            right_is_float = true;
        }
        if right_is_float
            && matches!(
                left,
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            )
        {
            left_is_float = true;
        }
        let dual_legacy = self.try_emit_legacy_dual_float_condition(left, right, double)?;
        let abs_pair = self.try_place_float_abs_pair_condition(left, right, double)?;
        let loaded_literal_live_argument = if right_literal && !left_literal {
            self.try_place_loaded_literal_with_live_float_argument(left, right, double)?
        } else {
            None
        };
        let product_literal = if right_literal && !left_literal {
            self.try_place_float_product_literal_condition(left, right, double)?
        } else {
            None
        };
        let loaded_pair_live_argument =
            self.try_place_loaded_pair_with_live_float_argument(left, right)?;
        let loaded_left_negated_loaded =
            self.try_place_loaded_left_negated_loaded_float_condition(left, right)?;
        let computed_left_loaded =
            self.try_place_computed_left_loaded_float_condition(left, right)?;
        let loaded_left_negated_leaf =
            self.try_place_loaded_left_negated_leaf_float_condition(left, right)?;
        let (a, b) = if let Some(registers) = dual_legacy {
            registers
        } else if let Some(registers) = abs_pair {
            registers
        } else if let Some(registers) = loaded_literal_live_argument {
            registers
        } else if let Some(registers) = product_literal {
            registers
        } else if let Some(registers) = loaded_pair_live_argument {
            registers
        } else if let Some(registers) = loaded_left_negated_loaded {
            registers
        } else if let Some(registers) = computed_left_loaded {
            registers
        } else if let Some(registers) = loaded_left_negated_leaf {
            registers
        } else if !left_is_float && right_is_float {
            // Usual arithmetic conversions promote the integer side to the
            // floating side's precision. A memory integer is first loaded into
            // r3, then uses the shared magic-bias conversion body in the current
            // structured frame. f2 keeps the bias clear of both compare values.
            let integer = 3;
            self.evaluate_general(left, integer)?;
            let signed = self.signedness_of(left)?;
            if signed && self.cast_operand_width(left).is_some_and(|width| width < 32) {
                let width = self.cast_operand_width(left).expect("checked");
                self.emit_widen(integer, integer, width as u8, true);
            }
            // The mixed comparison has its own measured schedule: start the
            // 0x4330 high word, issue the bias load, store the integer, overlap
            // the other memory operand, then assemble and subtract.
            let bias = if signed {
                0x4330_0000_8000_0000
            } else {
                0x4330_0000_0000_0000
            };
            if signed {
                self.output
                    .instructions
                    .push(Instruction::XorImmediateShifted {
                        a: integer,
                        s: integer,
                        immediate: 0x8000,
                    });
            }
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(0, 17200));
            self.load_double_constant(2, bias);
            let conversion_base = self.reserve_condition_conversion_scratch(1);
            self.output.instructions.push(Instruction::StoreWord {
                s: integer,
                a: 1,
                offset: conversion_base + 4,
            });
            self.evaluate_float(right, FLOAT_SCRATCH)?;
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: conversion_base,
            });
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: FLOAT_FIRST,
                a: 1,
                offset: conversion_base,
            });
            self.output.instructions.push(if double {
                Instruction::FloatSubtractDouble {
                    d: FLOAT_FIRST,
                    a: FLOAT_FIRST,
                    b: 2,
                }
            } else {
                Instruction::FloatSubtractSingle {
                    d: FLOAT_FIRST,
                    a: FLOAT_FIRST,
                    b: 2,
                }
            });
            self.output.has_conversion = true;
            self.frame_size = self.frame_size.max(16);
            (FLOAT_FIRST, FLOAT_SCRATCH)
        } else if left_is_float && !right_is_float {
            return Err(Diagnostic::error(
                "a right-side integer in a floating comparison needs the mixed FP scheduler (roadmap)",
            ));
        } else if left_load && right_load {
            // Two memory values use the two ordinary comparison temporaries:
            // the source-left value in f1 and source-right in f0. They are
            // loaded once each before the ordered/unordered compare.
            if self.f1_holds_float_argument() {
                // With a live float argument, evaluate the right subtree first
                // into an allocated home, then the left. This is MWCC's
                // heavier-memory-side-first schedule for paired ABS/computed
                // comparisons; liveness keeps f1 pinned and lets dead argument
                // homes re-enter the pool naturally.
                let b = self.place_float_compare_value(right)?;
                let a = self.place_float_compare_value(left)?;
                (a, b)
            } else {
                let a = self.place_condition_float_load(left, FLOAT_FIRST)?;
                let b = self.place_condition_float_load(right, FLOAT_SCRATCH)?;
                (a, b)
            }
        } else if eq && (left_load || right_load) {
            // `==`/`!=` against a loaded value (member/global) uses a *swapped* register
            // assignment versus the ordered form: the constant in f1 (loaded first), the
            // value in f0 — `lfs f1,k; lfs f0,(v); fcmpu f1,f0`. The == canonicalization
            // below (constant first) then emits the right operand order. f1 must be free
            // of a float argument; member==member and the like aren't modeled yet.
            if self.f1_holds_float_argument() {
                return Err(Diagnostic::error("a float == comparison with a float argument in f1 needs the FP register allocator (roadmap)"));
            }
            if right_literal && left_load {
                self.load_float_literal_into(FLOAT_FIRST, right, double)?;
                let value = self.place_condition_float_load(left, FLOAT_SCRATCH)?;
                (value, FLOAT_FIRST)
            } else if left_literal && right_load {
                self.load_float_literal_into(FLOAT_FIRST, left, double)?;
                let value = self.place_condition_float_load(right, FLOAT_SCRATCH)?;
                (FLOAT_FIRST, value)
            } else {
                return Err(Diagnostic::error("this floating-point == comparison needs the value register allocator (roadmap)"));
            }
        } else if left_load && !right_load && !right_literal {
            let b = self.float_register_of_leaf(right)?;
            let a = self.place_condition_float_load(left, FLOAT_SCRATCH)?;
            (a, b)
        } else if right_load && !left_load && !left_literal {
            let a = self.float_register_of_leaf(left)?;
            let b = self.place_condition_float_load(right, FLOAT_SCRATCH)?;
            (a, b)
        } else if right_literal && !left_literal {
            // One operand is a pool literal and the other a value that must be loaded (a
            // float member or global): mwcc loads the constant into f0 first, then the
            // value into f1 — `lfs f0,k; lfs f1,(v); fcmpo f1,f0`. Place the literal side
            // first so the loads emit in that order; the fcmpo keeps source order.
            // GC/2.0p1 and build 163 load the VALUE first (`lfs f1,(v); lfs f0,k`)
            // — same registers, reversed load order — so place the value side first there.
            if self.behavior.float_compare_value_before_const {
                let a = self.place_float_compare_value(left)?;
                let b = self.place_float_compare_operand(right, double)?;
                self.schedule_float_literal_in_dependent_load_gap();
                (a, b)
            } else {
                let b = self.place_float_compare_operand(right, double)?;
                let a = self.place_float_compare_value(left)?;
                (a, b)
            }
        } else if left_literal && !right_literal {
            if self.behavior.float_compare_value_before_const {
                let b = self.place_float_compare_value(right)?;
                let a = self.place_float_compare_operand(left, double)?;
                self.schedule_float_literal_in_dependent_load_gap();
                (a, b)
            } else {
                let a = self.place_float_compare_operand(left, double)?;
                let b = self.place_float_compare_value(right)?;
                (a, b)
            }
        } else {
            let a = self.place_float_compare_operand(left, double)?;
            let b = self.place_float_compare_operand(right, double)?;
            (a, b)
        };
        self.emit_structured_float_handoff_before_compare();
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            let (first, second) = if matches!(
                right,
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            ) {
                (b, a)
            } else {
                (a, b)
            };
            self.output
                .instructions
                .push(Instruction::FloatCompareUnordered {
                    a: first,
                    b: second,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::FloatCompareOrdered { a, b });
        }
        match operator {
            BinaryOperator::LessEqual => {
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterOr {
                        d: EQ,
                        a: LT,
                        b: EQ,
                    })
            }
            BinaryOperator::GreaterEqual => {
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterOr {
                        d: EQ,
                        a: GT,
                        b: EQ,
                    })
            }
            _ => {}
        }
        Ok(match operator {
            BinaryOperator::Less => (4, LT),
            BinaryOperator::Greater => (4, GT),
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual | BinaryOperator::Equal => {
                (4, EQ)
            }
            BinaryOperator::NotEqual => (12, EQ),
            _ => return Err(Diagnostic::error("unsupported floating-point condition")),
        })
    }

    /// Load a float or (promoted) integer literal into `dest` at the comparison's
    /// precision — `lfs`/`lfd` from the pool, the same promotion mwcc applies to a
    /// written `a > 0`.
    pub(crate) fn load_float_literal_into(
        &mut self,
        dest: u8,
        operand: &Expression,
        double: bool,
    ) -> Compilation<()> {
        let single_zero = !double
            && (matches!(operand, Expression::FloatLiteral(value) if *value as f32 == 0.0)
                || matches!(operand, Expression::IntegerLiteral(value) if *value == 0));
        let consumes_preload = self
            .preloaded_float_compare_literal
            .is_some_and(|preload| {
                preload.register == dest
                    && float_compare_literal_key(operand, double) == Some(preload.key)
            });
        if single_zero && self.condition_float_zero_register() == Some(dest) {
            if consumes_preload {
                self.preloaded_float_compare_literal = None;
            }
            return Ok(());
        }
        if consumes_preload {
            self.preloaded_float_compare_literal = None;
            return Ok(());
        }
        self.invalidate_condition_float_register(dest);
        match operand {
            Expression::FloatLiteral(value) => {
                if double {
                    self.load_double_constant(dest, value.to_bits());
                } else {
                    self.load_float_constant(dest, *value as f32);
                }
                Ok(())
            }
            Expression::IntegerLiteral(value) => {
                if double {
                    self.load_double_constant(dest, (*value as f64).to_bits());
                } else {
                    self.load_float_constant(dest, *value as f32);
                }
                Ok(())
            }
            _ => Err(Diagnostic::error("expected a float literal operand")),
        }?;
        if single_zero {
            self.record_condition_float_zero(dest);
        }
        Ok(())
    }

    /// Whether f1 currently holds a float argument (a float parameter lives there),
    /// so it can't double as the compare scratch without the FP register allocator.
    pub(crate) fn f1_holds_float_argument(&self) -> bool {
        self.locations
            .values()
            .any(|location| location.class == ValueClass::Float && location.register == 1)
    }

    /// Place a floating-point comparison operand: a leaf stays in its register; a
    /// float literal is loaded from the constant pool (`lfs`/`lfd`) into the float
    /// scratch, matching mwcc's `x > 0.0` form.
    fn place_float_compare_operand(
        &mut self,
        operand: &Expression,
        double: bool,
    ) -> Compilation<u8> {
        if matches!(
            operand,
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        ) {
            self.load_float_literal_into(FLOAT_SCRATCH, operand, double)?;
            return Ok(FLOAT_SCRATCH);
        }
        self.float_register_of_leaf(operand)
    }

    /// Place a float comparison operand that is a *value* (not a pool literal): a
    /// register leaf stays put; a float member/global/dereference is loaded into `f1`
    /// via evaluate_float (`lfs f1,(addr)`), the register mwcc uses for the compared
    /// value. Deferred when `f1` already holds a float argument — mwcc would pick a
    /// higher FPR there, which needs the register allocator.
    fn place_float_compare_value(&mut self, operand: &Expression) -> Compilation<u8> {
        const FLOAT_FIRST: u8 = 1;
        if self.is_float_leaf(operand) {
            return self.float_register_of_leaf(operand);
        }
        // A computed FP operand is not a member/global load. Give its result a
        // virtual home so the allocator can coalesce it with a dead input (the
        // leaf product can overwrite its factor) or preserve that input when it
        // remains live in the surrounding structured body. Direct loads retain
        // the measured fixed-register rules below.
        if matches!(
            operand,
            Expression::Binary { .. }
                | Expression::Unary { .. }
                | Expression::Cast { .. }
                | Expression::Conditional { .. }
        ) {
            if let Some(register) = self.try_place_cached_condition_arithmetic(operand) {
                return Ok(register);
            }
            let destination = self.fresh_virtual_float_preferring(FLOAT_FIRST);
            self.evaluate_float(operand, destination)?;
            return Ok(destination);
        }
        if crate::condition_float_cache::is_direct_float_memory_load(operand) {
            if let Some(register) = self.condition_float_register(operand) {
                self.record_condition_float_value(operand, register);
                return Ok(register);
            }
        }
        if let Some(register) = self.retained_float_compare_register(operand) {
            return Ok(register);
        }
        if self.f1_holds_float_argument() {
            return Err(Diagnostic::error("a float member/global compare with a float argument in f1 needs the FP register allocator (roadmap)"));
        }
        // A structured float local already lives in the virtual-register
        // allocator. Keep comparison temporaries in that same allocation
        // domain: hard-pinning this later load to f1 would conservatively
        // evict a long-lived local that MWCC leaves in f1, then require an
        // avoidable move back into the call-argument register.
        let destination = if self.has_virtual_float_location() {
            self.fresh_virtual_float_preferring(FLOAT_FIRST)
        } else {
            FLOAT_FIRST
        };
        self.place_condition_float_load(operand, destination)
    }

    pub(crate) fn place_condition_float_load(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<u8> {
        if let Some(register) = self.condition_float_register(operand) {
            self.record_condition_float_value(operand, register);
            return Ok(register);
        }
        self.invalidate_condition_float_register(destination);
        self.evaluate_float(operand, destination)?;
        self.record_condition_float_value(operand, destination);
        Ok(destination)
    }

    /// Whether `value` is a full-word (32-bit) memory load — a dereference,
    /// index, or struct member — which can be evaluated into a register and used
    /// as a comparison operand without narrow extension.
    pub(crate) fn is_word_load(&self, value: &Expression) -> bool {
        match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer) == Some(32),
            Expression::Index { base, .. } => self.dereferenced_width(base) == Some(32),
            Expression::Member { member_type, .. } => member_type.width() == 32,
            _ => false,
        }
    }

    /// A full-word load that is a SINGLE machine instruction — a dereference,
    /// struct member, or constant-index subscript (`lwz off(base)`). A
    /// variable-index subscript is excluded: it scales to `slwi; lwzx`, and the
    /// branchless comparison idioms then mis-schedule the constant/second-load
    /// against that two-instruction sequence (mwcc fills the `lwzx` latency gap
    /// with the independent `li`/load, which the scheduler does not reproduce),
    /// so those defer.
    pub(crate) fn is_simple_word_load(&self, value: &Expression) -> bool {
        self.is_word_load(value)
            && !matches!(value, Expression::Index { index, .. } if constant_value(index).is_none())
    }

    /// Whether `value` is an 8-bit memory load — a dereference, index, or struct
    /// member. All byte loads use `lbz`, so the loaded register already contains
    /// the exact unsigned-byte value even when the source type is signed.
    pub(crate) fn is_byte_load(&self, value: &Expression) -> bool {
        let width = match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer),
            Expression::Index { base, .. } => self.dereferenced_width(base),
            Expression::Member { member_type, .. } => Some(member_type.width()),
            _ => return false,
        };
        width == Some(8)
    }

    /// Whether `value` is a load of a signed 8-bit value (a `char`/`signed char`
    /// dereference, index, or struct member) — emitted as `lbz`, which zero-extends
    /// and so needs a following `extsb` in the sign-sensitive idioms.
    pub(crate) fn is_signed_byte_load(&self, value: &Expression) -> Compilation<bool> {
        Ok(self.is_byte_load(value) && self.signedness_of(value)?)
    }

    /// A NARROW (8/16-bit) UNSIGNED load (deref/element/member). It promotes to a SIGNED `int`
    /// before an arithmetic operator, so `(unsigned char/short) >> n` is an arithmetic `srawi`
    /// (mwcc), not the unsigned `srwi` ours would pick from the operand's own type — the loaded
    /// value is non-negative, so the result is identical but the instruction differs.
    pub(crate) fn is_narrow_unsigned_load(&self, value: &Expression) -> Compilation<bool> {
        let width = match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer),
            Expression::Index { base, .. } => self.dereferenced_width(base),
            Expression::Member { member_type, .. } => Some(member_type.width()),
            _ => return Ok(false),
        };
        Ok(matches!(width, Some(8) | Some(16)) && !self.signedness_of(value)?)
    }

    /// Place two leaf operands for the equality idiom, extending narrow operands
    /// the way mwcc does: when both are narrow the left is extended in its home
    /// register and the right into the scratch; when only one is narrow it goes to
    /// the scratch and the wide operand stays in its home register. Build-aware via
    /// each leaf's signedness; transparent (home registers) for the all-int case.
    pub(crate) fn place_compare_leaves(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<(u8, u8)> {
        // Two single-instruction full-word loads: mwcc loads the left operand into
        // a fresh register (the allocator colors it at the lowest free GPR) and the
        // right into the scratch, in source order — `lwz r4,…; lwz r0,…`. The
        // equality idiom that follows (`subf r0,r4,r0; cntlzw; srwi 5`) then
        // matches. Variable-index subscripts (scaled `slwi; lwzx`) are excluded —
        // two of them mis-schedule, so they defer.
        if self.is_simple_word_load(left) && self.is_simple_word_load(right) {
            let left_register = self.fresh_virtual_general();
            self.evaluate_general(left, left_register)?;
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            return Ok((left_register, GENERAL_SCRATCH));
        }
        let (left_register, left_width, left_signed) = self.leaf_info(left)?;
        let (right_register, right_width, right_signed) = self.leaf_info(right)?;
        let left_narrow = left_width < 32;
        let right_narrow = right_width < 32;

        let (left_placed, right_placed) = if left_narrow && right_narrow {
            self.emit_widen(left_register, left_register, left_width, left_signed);
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else if left_narrow {
            self.emit_widen(GENERAL_SCRATCH, left_register, left_width, left_signed);
            (GENERAL_SCRATCH, right_register)
        } else if right_narrow {
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else {
            (left_register, right_register)
        };
        Ok((left_placed, right_placed))
    }

    /// Place the two operands of a general signed comparison into registers. A
    /// leaf stays in its home register; a single non-leaf operand is computed
    /// somewhere the idiom can keep it live across its two uses — and *where*
    /// depends on the idiom:
    ///
    ///  - `<`/`>` interleave the scratch (holding `a^b`) with the result-path
    ///    temp, so the non-leaf operand must survive in a *preserved* register
    ///    that AVOIDS the destination, leaving the destination free for that temp
    ///    (mwcc's coalescing — p->a > x, x < p->a).
    ///  - `!=` uses the operand in two ADJACENT subtractions with nothing
    ///    competing for the scratch between them, so mwcc evaluates it straight
    ///    into the *scratch* (r0) and lets the second subtraction overwrite it
    ///    (x != p->a, x != a*b+c). Deeper operands borrow the destination as an
    ///    internal temp but still settle into the scratch.
    ///
    /// Two non-leaf operands are not handled here.
    /// The register holding a comparison's right operand: a leaf in its home
    /// register, or a constant materialized into the scratch (`li r0, C`). The
    /// caller's idiom must read the right operand before overwriting r0.
    fn compare_right_operand(&mut self, right: &Expression) -> Compilation<u8> {
        if let Some(constant) = constant_value(right) {
            self.load_integer_constant(GENERAL_SCRATCH, constant);
            Ok(GENERAL_SCRATCH)
        } else {
            self.general_register_of_leaf(right)
        }
    }

    fn place_compare_operands(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<(u8, u8)> {
        let left_leaf = leaf_name(left).is_some();
        let right_leaf = leaf_name(right).is_some();
        if left_leaf && right_leaf {
            return Ok((
                self.general_register_of_leaf(left)?,
                self.general_register_of_leaf(right)?,
            ));
        }
        if matches!(operator, BinaryOperator::NotEqual) {
            // The non-leaf operand goes into the scratch; the leaf keeps its home.
            return if left_leaf {
                let left_register = self.general_register_of_leaf(left)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Ok((left_register, GENERAL_SCRATCH))
            } else {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                Ok((GENERAL_SCRATCH, self.general_register_of_leaf(right)?))
            };
        }
        match (left_leaf, right_leaf) {
            // Non-leaf LEFT (the `>` idiom keeps its left): evaluate it off the dest.
            (false, true) => {
                let right_register = self.general_register_of_leaf(right)?;
                let left_register = self.fresh_virtual_general_avoiding(vec![destination]);
                self.evaluate_general(left, left_register)?;
                Ok((left_register, right_register))
            }
            // Non-leaf RIGHT (the `<` idiom keeps its right): evaluate it off the dest.
            (true, false) => {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.fresh_virtual_general_avoiding(vec![destination]);
                self.evaluate_general(right, right_register)?;
                Ok((left_register, right_register))
            }
            // Two non-leaf operands are not handled yet.
            _ => Err(Diagnostic::error(
                "this comparison operand shape needs the full register allocator (roadmap)",
            )),
        }
    }
}
