//! Variable-index pointer read/modify/write lowering.

use super::*;

impl Generator {
    /// Emit a variable-index word read/modify/write with a leaf or encodable
    /// constant right-hand side. `indexed_update_syntax` retains the frontend
    /// distinction between `a[i] op= rhs`/`a[i]++` and the explicitly spelled
    /// `a[i] = a[i] op rhs`, which affects operand order in every generation and
    /// the address form in build 163.
    pub(crate) fn try_emit_indexed_rmw(
        &mut self,
        target: &Expression,
        value: &Expression,
        indexed_update_syntax: bool,
    ) -> Compilation<bool> {
        use BinaryOperator::*;
        let Expression::Index { base, index } = target else {
            return Ok(false);
        };
        if leaf_name(base).is_none() || constant_value(index).is_some() {
            return Ok(false);
        }
        let Expression::Binary {
            operator,
            left,
            right,
        } = value
        else {
            return Ok(false);
        };
        if !matches!(
            operator,
            Add | Subtract | BitAnd | BitOr | BitXor | Multiply
        ) {
            return Ok(false);
        }
        // The modified value must read the very same element being stored.
        if !same_operand(target, left) {
            return Ok(false);
        }
        let (pointee, address) = self.resolve_pointer(base)?;
        if !matches!(pointee, Pointee::Int | Pointee::UnsignedInt) {
            return Ok(false);
        }
        let index_register = self.general_register_of_leaf(index)?;
        let size_shift = pointee.size().trailing_zeros() as u8;
        let scratch = GENERAL_SCRATCH;

        if !indexed_update_syntax
            && self.behavior.indexed_rmw_assignment_style
                == mwcc_versions::IndexedRmwAssignmentStyle::PreserveExplicitAddress
            && (self.plain_integer_leaf_register(right).is_some()
                || constant_value(right)
                    .is_some_and(|constant| indexed_rmw_constant_is_encodable(*operator, constant)))
        {
            self.emit_explicit_address_indexed_rmw(
                pointee,
                address,
                index_register,
                size_shift,
                *operator,
                right,
            )?;
            return Ok(true);
        }

        // With update syntax the loaded value is the first commutative operand;
        // explicit assignment reverses those operands even when both versions
        // otherwise use the same indexed load/store sequence.
        if let Some(rhs_register) = self.plain_integer_leaf_register(right) {
            let scaled = self.fresh_virtual_general();
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index_register,
                    shift: size_shift,
                });
            self.output
                .instructions
                .push(indexed_load(pointee, scratch, address, scaled)?);
            let combined = match operator {
                Add => Instruction::Add {
                    d: scratch,
                    a: if indexed_update_syntax {
                        scratch
                    } else {
                        rhs_register
                    },
                    b: if indexed_update_syntax {
                        rhs_register
                    } else {
                        scratch
                    },
                },
                Subtract => Instruction::SubtractFrom {
                    d: scratch,
                    a: rhs_register,
                    b: scratch,
                },
                Multiply => Instruction::MultiplyLow {
                    d: scratch,
                    a: if indexed_update_syntax {
                        scratch
                    } else {
                        rhs_register
                    },
                    b: if indexed_update_syntax {
                        rhs_register
                    } else {
                        scratch
                    },
                },
                BitAnd => Instruction::And {
                    a: scratch,
                    s: if indexed_update_syntax {
                        scratch
                    } else {
                        rhs_register
                    },
                    b: if indexed_update_syntax {
                        rhs_register
                    } else {
                        scratch
                    },
                },
                BitOr => Instruction::Or {
                    a: scratch,
                    s: if indexed_update_syntax {
                        scratch
                    } else {
                        rhs_register
                    },
                    b: if indexed_update_syntax {
                        rhs_register
                    } else {
                        scratch
                    },
                },
                _ => Instruction::Xor {
                    a: scratch,
                    s: if indexed_update_syntax {
                        scratch
                    } else {
                        rhs_register
                    },
                    b: if indexed_update_syntax {
                        rhs_register
                    } else {
                        scratch
                    },
                },
            };
            self.output.instructions.push(combined);
            self.output
                .instructions
                .push(indexed_store(pointee, scratch, address, scaled)?);
            return Ok(true);
        }

        // `a[i] += C` / `a[i] -= C` / `a[i]++` (a constant addend that fits an
        // immediate): mwcc loads the value into a register (not the scratch) and
        // the `addi` targets the scratch — `slwi r5,i,2; lwzx r4,base,r5; addi
        // r0,r4,C; stwx r0,base,r5`. Both the scaled index and the loaded value
        // are virtuals; at the `slwi` the index source is still live, so the
        // allocator places the index above the value, reproducing mwcc.
        if matches!(operator, Add | Subtract) {
            let immediate = constant_value(right)
                .and_then(|c| {
                    if matches!(operator, Subtract) {
                        c.checked_neg()
                    } else {
                        Some(c)
                    }
                })
                .and_then(|c| i16::try_from(c).ok());
            if let Some(immediate) = immediate {
                // The scaled index avoids the index register so the loaded value
                // (not the index) coalesces onto the now-dead index register —
                // mwcc's `slwi r5,i,2; lwzx r4,…` rather than the reverse.
                let scaled = self.fresh_virtual_general_avoiding(vec![index_register]);
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: scaled,
                        s: index_register,
                        shift: size_shift,
                    });
                let loaded = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(indexed_load(pointee, loaded, address, scaled)?);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: scratch,
                    a: loaded,
                    immediate,
                });
                self.output
                    .instructions
                    .push(indexed_store(pointee, scratch, address, scaled)?);
                return Ok(true);
            }
        }

        // `a[i] |= C` / `^= C` / `&= C` / `*= C`: the loaded value flows through
        // the scratch and the op is an in-place immediate (`ori`/`xori`/`mulli`,
        // or `rlwinm` for a contiguous-mask AND) — the leaf-shape coloring, so the
        // scaled index coalesces onto the dead index register.
        if let Some(constant) = constant_value(right) {
            let immediate_op = match operator {
                BitOr if u16::try_from(constant).is_ok() => Instruction::OrImmediate {
                    a: scratch,
                    s: scratch,
                    immediate: constant as u16,
                },
                BitXor if u16::try_from(constant).is_ok() => Instruction::XorImmediate {
                    a: scratch,
                    s: scratch,
                    immediate: constant as u16,
                },
                // `a[i] *= 2^k` strength-reduces to a left shift, like every other multiply
                // context (`slwi r0,r0,k`), NOT `mulli`; a non-power-of-two keeps `mulli`.
                Multiply if constant > 1 && (constant & (constant - 1)) == 0 => {
                    Instruction::ShiftLeftImmediate {
                        a: scratch,
                        s: scratch,
                        shift: constant.trailing_zeros() as u8,
                    }
                }
                Multiply if i16::try_from(constant).is_ok() => Instruction::MultiplyImmediate {
                    d: scratch,
                    a: scratch,
                    immediate: constant as i16,
                },
                BitAnd => match rlwinm_mask(constant) {
                    Some((begin, end)) => Instruction::RotateAndMask {
                        a: scratch,
                        s: scratch,
                        shift: 0,
                        begin,
                        end,
                    },
                    None => return Ok(false),
                },
                _ => return Ok(false),
            };
            let scaled = self.fresh_virtual_general();
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index_register,
                    shift: size_shift,
                });
            self.output
                .instructions
                .push(indexed_load(pointee, scratch, address, scaled)?);
            self.output.instructions.push(immediate_op);
            self.output
                .instructions
                .push(indexed_store(pointee, scratch, address, scaled)?);
            return Ok(true);
        }
        Ok(false)
    }

    /// Build 163 retains the frontend shape of an explicitly spelled
    /// `a[i] = a[i] op rhs`: scale into r0, form one element address, and use
    /// displacement-zero load/store instructions. Compound assignment takes the
    /// ordinary indexed path above even when the arithmetic is identical.
    fn emit_explicit_address_indexed_rmw(
        &mut self,
        pointee: Pointee,
        base: u8,
        index: u8,
        size_shift: u8,
        operator: BinaryOperator,
        right: &Expression,
    ) -> Compilation<()> {
        use BinaryOperator::*;
        let scratch = GENERAL_SCRATCH;
        let add_immediate = if matches!(operator, Add | Subtract) {
            constant_value(right)
                .and_then(|constant| {
                    if operator == Subtract {
                        constant.checked_neg()
                    } else {
                        Some(constant)
                    }
                })
                .and_then(|constant| i16::try_from(constant).ok())
        } else {
            None
        };
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scratch,
                s: index,
                shift: size_shift,
            });
        let address = if add_immediate.is_some() {
            self.fresh_virtual_general_avoiding(vec![base])
        } else {
            self.fresh_virtual_general()
        };
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: base,
            b: scratch,
        });

        if let Some(immediate) = add_immediate {
            let loaded = self.fresh_virtual_general();
            self.output
                .instructions
                .push(displacement_load(pointee, loaded, address, 0)?);
            self.output.instructions.push(Instruction::AddImmediate {
                d: scratch,
                a: loaded,
                immediate,
            });
            self.output
                .instructions
                .push(displacement_store(pointee, scratch, address, 0)?);
            return Ok(());
        }

        self.output
            .instructions
            .push(displacement_load(pointee, scratch, address, 0)?);
        let operation = if let Some(rhs) = self.plain_integer_leaf_register(right) {
            match operator {
                Add => Instruction::Add {
                    d: scratch,
                    a: rhs,
                    b: scratch,
                },
                Subtract => Instruction::SubtractFrom {
                    d: scratch,
                    a: rhs,
                    b: scratch,
                },
                Multiply => Instruction::MultiplyLow {
                    d: scratch,
                    a: rhs,
                    b: scratch,
                },
                BitAnd => Instruction::And {
                    a: scratch,
                    s: rhs,
                    b: scratch,
                },
                BitOr => Instruction::Or {
                    a: scratch,
                    s: rhs,
                    b: scratch,
                },
                BitXor => Instruction::Xor {
                    a: scratch,
                    s: rhs,
                    b: scratch,
                },
                _ => unreachable!("indexed RMW operator was validated by the caller"),
            }
        } else if let Some(constant) = constant_value(right) {
            match operator {
                BitOr if u16::try_from(constant).is_ok() => Instruction::OrImmediate {
                    a: scratch,
                    s: scratch,
                    immediate: constant as u16,
                },
                BitXor if u16::try_from(constant).is_ok() => Instruction::XorImmediate {
                    a: scratch,
                    s: scratch,
                    immediate: constant as u16,
                },
                Multiply if constant > 1 && (constant & (constant - 1)) == 0 => {
                    Instruction::ShiftLeftImmediate {
                        a: scratch,
                        s: scratch,
                        shift: constant.trailing_zeros() as u8,
                    }
                }
                Multiply if i16::try_from(constant).is_ok() => Instruction::MultiplyImmediate {
                    d: scratch,
                    a: scratch,
                    immediate: constant as i16,
                },
                BitAnd => match rlwinm_mask(constant) {
                    Some((begin, end)) => Instruction::RotateAndMask {
                        a: scratch,
                        s: scratch,
                        shift: 0,
                        begin,
                        end,
                    },
                    None => return Err(Diagnostic::error("indexed RMW mask is not encodable")),
                },
                _ => return Err(Diagnostic::error("indexed RMW immediate is not encodable")),
            }
        } else {
            return Err(Diagnostic::error(
                "computed explicit-address indexed RMW needs allocator support (roadmap)",
            ));
        };
        self.output.instructions.push(operation);
        self.output
            .instructions
            .push(displacement_store(pointee, scratch, address, 0)?);
        Ok(())
    }
}

fn indexed_rmw_constant_is_encodable(operator: BinaryOperator, constant: i64) -> bool {
    use BinaryOperator::*;
    match operator {
        Add => i16::try_from(constant).is_ok(),
        Subtract => constant
            .checked_neg()
            .is_some_and(|value| i16::try_from(value).is_ok()),
        BitOr | BitXor => u16::try_from(constant).is_ok(),
        BitAnd => rlwinm_mask(constant).is_some(),
        Multiply => {
            (constant > 1 && (constant & (constant - 1)) == 0) || i16::try_from(constant).is_ok()
        }
        _ => false,
    }
}
