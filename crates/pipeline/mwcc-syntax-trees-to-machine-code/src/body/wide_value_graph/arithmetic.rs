//! Arithmetic policy and selection for typed word/pair graph values.
use super::*;

pub(super) fn fold(operator: BinaryOperator, ty: Type, left: u64, right: u64) -> Option<u64> {
    let signed = |bits| {
        if wide(ty) {
            bits as i64
        } else {
            bits as i32 as i64
        }
    };
    let bits = match operator {
        BinaryOperator::Add => left.wrapping_add(right),
        BinaryOperator::Subtract => left.wrapping_sub(right),
        BinaryOperator::Multiply => left.wrapping_mul(right),
        BinaryOperator::BitAnd => left & right,
        BinaryOperator::BitOr => left | right,
        BinaryOperator::BitXor => left ^ right,
        BinaryOperator::Divide | BinaryOperator::Modulo if right == 0 => return None,
        BinaryOperator::Divide if ty.is_signed() => signed(left).wrapping_div(signed(right)) as u64,
        BinaryOperator::Modulo if ty.is_signed() => signed(left).wrapping_rem(signed(right)) as u64,
        BinaryOperator::Divide => left / right,
        BinaryOperator::Modulo => left % right,
        BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
            if right >= if wide(ty) { 64 } else { 32 } =>
        {
            return None
        }
        BinaryOperator::ShiftLeft => left << right,
        BinaryOperator::ShiftRight if ty.is_signed() => (signed(left) >> right) as u64,
        BinaryOperator::ShiftRight => left >> right,
        BinaryOperator::Equal => u64::from(left == right),
        BinaryOperator::NotEqual => u64::from(left != right),
        op if is_comparison(op) => {
            let ordering = if ty.is_signed() {
                signed(left).cmp(&signed(right))
            } else {
                left.cmp(&right)
            };
            u64::from(match op {
                BinaryOperator::Less => ordering.is_lt(),
                BinaryOperator::LessEqual => !ordering.is_gt(),
                BinaryOperator::Greater => ordering.is_gt(),
                BinaryOperator::GreaterEqual => !ordering.is_lt(),
                _ => unreachable!(),
            })
        }
        _ => return None,
    };
    Some(if wide(ty) { bits } else { bits & 0xffff_ffff })
}

impl Generator {
    pub(super) fn emit_wide_graph_arithmetic(
        &mut self,
        result: Value,
        operator: BinaryOperator,
        left: Value,
        right: Value,
        registers: &mut [Option<Registers>],
    ) -> Compilation<()> {
        if !wide(left.ty)
            && !matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
            )
        {
            return self.emit_wide_graph_scalar(result, operator, left, right, registers);
        }
        let left_type = left.ty;
        let shift = match right.source {
            Source::Constant(n) => Some(n as u8),
            _ => None,
        };
        let left = self.wide_graph_operand(left, registers);
        let right = self.wide_graph_operand(right, registers);
        let destination = self.wide_graph_destination(result, registers);
        if is_comparison(operator) {
            let truth = self.fresh_label();
            let done = self.fresh_label();
            self.load_integer_constant(destination.low, 0);
            self.output.instructions.push(if left_type.is_signed() {
                Instruction::CompareWord {
                    a: left.high.unwrap(),
                    b: right.high.unwrap(),
                }
            } else {
                Instruction::CompareLogicalWord {
                    a: left.high.unwrap(),
                    b: right.high.unwrap(),
                }
            });
            match operator {
                BinaryOperator::Equal => self.emit_branch_conditional_to(4, 2, done),
                BinaryOperator::NotEqual => self.emit_branch_conditional_to(4, 2, truth),
                BinaryOperator::Less | BinaryOperator::LessEqual => {
                    self.emit_branch_conditional_to(12, 0, truth);
                    self.emit_branch_conditional_to(12, 1, done);
                }
                _ => {
                    self.emit_branch_conditional_to(12, 1, truth);
                    self.emit_branch_conditional_to(12, 0, done);
                }
            }
            self.output
                .instructions
                .push(Instruction::CompareLogicalWord {
                    a: left.low,
                    b: right.low,
                });
            let (options, bit) = match operator {
                BinaryOperator::Equal => (12, 2),
                BinaryOperator::NotEqual => (4, 2),
                BinaryOperator::Less => (12, 0),
                BinaryOperator::LessEqual => (4, 1),
                BinaryOperator::Greater => (12, 1),
                _ => (4, 0),
            };
            self.emit_branch_conditional_to(options, bit, truth);
            self.emit_branch_to(done);
            self.bind_label(truth);
            self.load_integer_constant(destination.low, 1);
            self.bind_label(done);
            return Ok(());
        }
        if operator == BinaryOperator::Multiply {
            let high = destination.high.unwrap();
            let cross_left = self.fresh_virtual_general();
            let cross_right = self.fresh_virtual_general();
            let upper = self.fresh_virtual_general();
            let sum = self.fresh_virtual_general();
            self.output.instructions.extend([
                Instruction::MultiplyLow {
                    d: cross_left,
                    a: left.high.unwrap(),
                    b: right.low,
                },
                Instruction::MultiplyLow {
                    d: cross_right,
                    a: left.low,
                    b: right.high.unwrap(),
                },
                Instruction::MultiplyHighWordUnsigned {
                    d: upper,
                    a: left.low,
                    b: right.low,
                },
                Instruction::Add {
                    d: sum,
                    a: cross_left,
                    b: cross_right,
                },
                Instruction::Add {
                    d: high,
                    a: sum,
                    b: upper,
                },
                Instruction::MultiplyLow {
                    d: destination.low,
                    a: left.low,
                    b: right.low,
                },
            ]);
            return Ok(());
        }
        if matches!(
            operator,
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
        ) {
            let n = shift.expect("constant pair shift");
            let high = destination.high.unwrap();
            let source_high = left.high.unwrap();
            if n == 0 {
                self.output.instructions.extend([
                    Instruction::move_register(high, source_high),
                    Instruction::move_register(destination.low, left.low),
                ]);
            } else if operator == BinaryOperator::ShiftLeft {
                if n < 32 {
                    let cross = self.fresh_virtual_general();
                    let top = self.fresh_virtual_general();
                    self.output.instructions.extend([
                        Instruction::ShiftRightLogicalImmediate {
                            a: cross,
                            s: left.low,
                            shift: 32 - n,
                        },
                        Instruction::ShiftLeftImmediate {
                            a: top,
                            s: source_high,
                            shift: n,
                        },
                        Instruction::Or {
                            a: high,
                            s: top,
                            b: cross,
                        },
                        Instruction::ShiftLeftImmediate {
                            a: destination.low,
                            s: left.low,
                            shift: n,
                        },
                    ]);
                } else {
                    if n == 32 {
                        self.output
                            .instructions
                            .push(Instruction::move_register(high, left.low));
                    } else {
                        self.output
                            .instructions
                            .push(Instruction::ShiftLeftImmediate {
                                a: high,
                                s: left.low,
                                shift: n - 32,
                            });
                    }
                    self.load_integer_constant(destination.low, 0);
                }
            } else {
                let upper_shift = |a, n| {
                    if left_type.is_signed() {
                        Instruction::ShiftRightAlgebraicImmediate {
                            a,
                            s: source_high,
                            shift: n,
                        }
                    } else {
                        Instruction::ShiftRightLogicalImmediate {
                            a,
                            s: source_high,
                            shift: n,
                        }
                    }
                };
                if n < 32 {
                    let cross = self.fresh_virtual_general();
                    let bottom = self.fresh_virtual_general();
                    self.output.instructions.extend([
                        Instruction::ShiftLeftImmediate {
                            a: cross,
                            s: source_high,
                            shift: 32 - n,
                        },
                        Instruction::ShiftRightLogicalImmediate {
                            a: bottom,
                            s: left.low,
                            shift: n,
                        },
                        Instruction::Or {
                            a: destination.low,
                            s: bottom,
                            b: cross,
                        },
                        upper_shift(high, n),
                    ]);
                } else {
                    if n == 32 {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination.low, source_high));
                    } else {
                        self.output
                            .instructions
                            .push(upper_shift(destination.low, n - 32));
                    }
                    if left_type.is_signed() {
                        self.output.instructions.push(upper_shift(high, 31));
                    } else {
                        self.load_integer_constant(high, 0);
                    }
                }
            }
            return Ok(());
        }
        let instruction = |d, a, b, high| match operator {
            BinaryOperator::Add if high => Instruction::AddExtended { d, a, b },
            BinaryOperator::Add if destination.high.is_some() => {
                Instruction::AddCarrying { d, a, b }
            }
            BinaryOperator::Add => Instruction::Add { d, a, b },
            BinaryOperator::Subtract if high => Instruction::SubtractFromExtended { d, a: b, b: a },
            BinaryOperator::Subtract if destination.high.is_some() => {
                Instruction::SubtractFromCarrying { d, a: b, b: a }
            }
            BinaryOperator::Subtract => Instruction::SubtractFrom { d, a: b, b: a },
            BinaryOperator::BitAnd => Instruction::And { a: d, s: a, b },
            BinaryOperator::BitOr => Instruction::Or { a: d, s: a, b },
            BinaryOperator::BitXor => Instruction::Xor { a: d, s: a, b },
            _ => unreachable!(),
        };
        self.output
            .instructions
            .push(instruction(destination.low, left.low, right.low, false));
        if let Some(high) = destination.high {
            self.output.instructions.push(instruction(
                high,
                left.high.unwrap(),
                right.high.unwrap(),
                true,
            ));
        }
        Ok(())
    }

    fn emit_wide_graph_scalar(
        &mut self,
        result: Value,
        operator: BinaryOperator,
        left: Value,
        right: Value,
        registers: &mut [Option<Registers>],
    ) -> Compilation<()> {
        let destination = self.wide_graph_destination(result, registers).low;
        let mut bindings = Vec::new();
        let mut operands = Vec::new();
        for value in [left, right] {
            // Scalar comparison idioms accept signed 16-bit immediates. Wider
            // constants need an ordinary virtual operand, just like a load.
            let immediate = match value.source {
                Source::Constant(bits)
                    if !is_comparison(operator) || i16::try_from(bits as i32).is_ok() =>
                {
                    Some(bits)
                }
                _ => None,
            };
            if let Some(bits) = immediate {
                let literal = Expression::IntegerLiteral(if value.ty.is_signed() {
                    bits as i32 as i64
                } else {
                    bits as i64
                });
                operands.push(if value.ty.is_signed() {
                    literal
                } else {
                    Expression::Cast {
                        target_type: Type::UnsignedInt,
                        operand: Box::new(literal),
                    }
                });
            } else {
                let home = self.wide_graph_operand(value, registers).low;
                let mut name = format!("__mwcc_graph_{home}");
                while self.locations.contains_key(&name)
                    || self.globals.contains_key(&name)
                    || self.known_locals.contains(&name)
                {
                    name.push('_');
                }
                self.locations.insert(
                    name.clone(),
                    Location {
                        class: ValueClass::General,
                        register: home,
                        signed: value.ty.is_signed(),
                        width: 32,
                        pointee: None,
                        stride: None,
                    },
                );
                let restore = self.reserved.insert(home);
                bindings.push((name.clone(), home, restore));
                operands.push(Expression::Variable(name));
            }
        }
        let expression = Expression::Binary {
            operator,
            left: Box::new(operands.remove(0)),
            right: Box::new(operands.remove(0)),
        };
        let result = self.evaluate_general(&expression, destination);
        for (name, home, restore) in bindings {
            self.locations.remove(&name);
            if restore {
                self.reserved.remove(&home);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folding_preserves_signed_division_and_word_width() {
        assert_eq!(
            fold(BinaryOperator::Divide, Type::LongLong, (-17i64) as u64, 3),
            Some((-5i64) as u64)
        );
        assert_eq!(
            fold(BinaryOperator::Modulo, Type::LongLong, (-17i64) as u64, 3),
            Some((-2i64) as u64)
        );
        assert_eq!(
            fold(
                BinaryOperator::Multiply,
                Type::UnsignedLongLong,
                0xffffffff,
                0xffffffff
            ),
            Some(0xfffffffe00000001)
        );
        assert_eq!(
            fold(
                BinaryOperator::Multiply,
                Type::UnsignedInt,
                0xffffffff,
                0xffffffff
            ),
            Some(1)
        );
        assert_eq!(fold(BinaryOperator::Divide, Type::LongLong, 5, 0), None);
    }
    #[test]
    fn comparison_and_shift_folding_respect_the_operand_domain() {
        assert_eq!(
            fold(BinaryOperator::Less, Type::LongLong, u64::MAX, 0),
            Some(1)
        );
        assert_eq!(
            fold(BinaryOperator::Less, Type::UnsignedLongLong, u64::MAX, 0),
            Some(0)
        );
        assert_eq!(
            fold(
                BinaryOperator::ShiftRight,
                Type::LongLong,
                0x8000000000000000,
                63
            ),
            Some(u64::MAX)
        );
        assert_eq!(
            fold(
                BinaryOperator::ShiftRight,
                Type::UnsignedLongLong,
                0x8000000000000000,
                63
            ),
            Some(1)
        );
        assert_eq!(
            fold(BinaryOperator::ShiftLeft, Type::UnsignedLongLong, 1, 64),
            None
        );
        assert_eq!(
            fold(BinaryOperator::ShiftLeft, Type::UnsignedInt, 1, 32),
            None
        );
    }
}

impl Graph<'_> {
    pub(super) fn unary(&mut self, operator: UnaryOperator, operand: &Expression) -> Option<Value> {
        let value = self.expression(operand)?;
        let ty = if value.ty.width() < 32 {
            Type::Int
        } else {
            value.ty
        };
        let value = self.convert(value, ty)?;
        if matches!(ty, Type::Pointer(_) | Type::StructPointer { .. })
            && operator != UnaryOperator::LogicalNot
        {
            return None;
        }
        let (operator, left, right) = match operator {
            UnaryOperator::Negate => (
                BinaryOperator::Subtract,
                Value {
                    ty,
                    source: Source::Constant(0),
                },
                value,
            ),
            UnaryOperator::BitNot => (
                BinaryOperator::BitXor,
                value,
                Value {
                    ty,
                    source: Source::Constant(if wide(ty) { u64::MAX } else { u32::MAX as u64 }),
                },
            ),
            UnaryOperator::LogicalNot => (
                BinaryOperator::Equal,
                value,
                Value {
                    ty,
                    source: Source::Constant(0),
                },
            ),
        };
        let result_type = if is_comparison(operator) {
            Type::Int
        } else {
            ty
        };
        if let (Source::Constant(left), Source::Constant(right)) = (left.source, right.source) {
            return Some(Value {
                ty: result_type,
                source: Source::Constant(fold(operator, ty, left, right)?),
            });
        }
        let result = self.fresh(result_type);
        self.operations.push(Operation::Binary {
            result,
            operator,
            left,
            right,
        });
        Some(result)
    }
}
