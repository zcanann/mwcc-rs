//! Fixed-port header writes followed by an eight-way modulo-scheduled zero fill.

#[allow(unused_imports)]
use super::*;

fn fixed_port_target(target: &Expression) -> Option<(u32, Type)> {
    match target {
        Expression::Member {
            base,
            offset: 0,
            member_type,
            index_stride: None,
        } => {
            let Expression::Cast {
                target_type: Type::StructPointer { .. },
                operand,
            } = base.as_ref()
            else {
                return None;
            };
            let address = constant_value(operand).and_then(|value| u32::try_from(value).ok())?;
            Some((address, *member_type))
        }
        Expression::Dereference { pointer } => {
            let Expression::Cast {
                target_type: Type::Pointer(pointee),
                operand,
            } = pointer.as_ref()
            else {
                return None;
            };
            let width = match pointee {
                Pointee::Int => Type::Int,
                Pointee::UnsignedInt => Type::UnsignedInt,
                Pointee::UnsignedChar => Type::UnsignedChar,
                Pointee::UnsignedShort => Type::UnsignedShort,
                _ => return None,
            };
            let address = constant_value(operand).and_then(|value| u32::try_from(value).ok())?;
            Some((address, width))
        }
        _ => None,
    }
}

impl Generator {
    /// Lower a fixed-port flush family:
    ///
    /// ```text
    /// size = state->width * state->height;
    /// PORT8 = command; PORT16 = state->width;
    /// for (i = 0; i < size; i += 4) PORT32 = 0;
    /// state->flushed = 1;
    /// ```
    ///
    /// Build 163 rounds the byte count to words, then emits an eight-store CTR loop and a
    /// remainder CTR loop. This owns the complete schedule because the header loads and stores are
    /// interleaved with the loop-count preparation.
    pub(crate) fn try_fixed_port_zero_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let (counter, width_local, size) = match function.locals.as_slice() {
            [counter, size] => (counter, None, size),
            [counter, width, size] => (counter, Some(width), size),
            _ => return Ok(false),
        };
        if counter.declared_type != Type::UnsignedInt
            || counter.initializer.is_some()
            || counter.array_length.is_some()
            || counter.is_static
            || size.declared_type != Type::UnsignedInt
            || size.array_length.is_some()
            || size.is_static
        {
            return Ok(false);
        }
        let Some(Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: size_left,
            right: size_right,
        }) = size.initializer.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: right_base,
            offset: right_offset,
            member_type: Type::UnsignedShort,
            index_stride: None,
        } = size_right.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(right_global) = right_base.as_ref() else {
            return Ok(false);
        };
        let (global, left_offset, width_name) = if let Some(width) = width_local {
            if width.declared_type != Type::UnsignedShort
                || width.array_length.is_some()
                || width.is_static
                || !matches!(size_left.as_ref(), Expression::Variable(name) if name == &width.name)
            {
                return Ok(false);
            }
            let Some(Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            }) = width.initializer.as_ref()
            else {
                return Ok(false);
            };
            let Expression::Variable(global) = base.as_ref() else {
                return Ok(false);
            };
            (global, offset, Some(width.name.as_str()))
        } else {
            let Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            } = size_left.as_ref()
            else {
                return Ok(false);
            };
            let Expression::Variable(global) = base.as_ref() else {
                return Ok(false);
            };
            (global, offset, None)
        };
        if global != right_global {
            return Ok(false);
        }
        let (Ok(left_displacement), Ok(right_displacement)) =
            (i16::try_from(*left_offset), i16::try_from(*right_offset))
        else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };

        let [command_store, extent_store, loop_statement, trailing_store] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Statement::Store {
            target: command_target,
            value: command_value,
        } = command_store
        else {
            return Ok(false);
        };
        let Some(command) = constant_value(command_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some((port, Type::UnsignedChar)) = fixed_port_target(command_target) else {
            return Ok(false);
        };
        let Statement::Store {
            target: extent_target,
            value: extent_value,
        } = extent_store
        else {
            return Ok(false);
        };
        let Some((extent_port, Type::UnsignedShort)) = fixed_port_target(extent_target) else {
            return Ok(false);
        };
        let extent_matches = match width_name {
            Some(width) => matches!(extent_value, Expression::Variable(name) if name == width),
            None => matches!(extent_value, Expression::Member { base, offset, member_type: Type::UnsignedShort, index_stride: None }
                if *offset == *left_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == global)),
        };
        if extent_port != port || !extent_matches {
            return Ok(false);
        }

        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        } = loop_statement
        else {
            return Ok(false);
        };
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && constant_value(value) == Some(0))
            || !matches!(condition, Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name)
                    && matches!(right.as_ref(), Expression::Variable(name) if name == &size.name))
            || !matches!(step, Expression::Assign { target, value }
                if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                    && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                        if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name)
                            && constant_value(right) == Some(4)))
        {
            return Ok(false);
        }
        let [Statement::Store {
            target: zero_target,
            value: zero_value,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        let Some((zero_port, zero_type)) = fixed_port_target(zero_target) else {
            return Ok(false);
        };
        if zero_port != port
            || !matches!(zero_type, Type::Int | Type::UnsignedInt)
            || constant_value(zero_value) != Some(0)
        {
            return Ok(false);
        }

        let Statement::Store {
            target:
                Expression::Member {
                    base: trailing_base,
                    offset: trailing_offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                },
            value: trailing_value,
        } = trailing_store
        else {
            return Ok(false);
        };
        let Ok(trailing_displacement) = i16::try_from(*trailing_offset) else {
            return Ok(false);
        };
        if !matches!(trailing_base.as_ref(), Expression::Variable(name) if name == global)
            || constant_value(trailing_value) != Some(1)
        {
            return Ok(false);
        }

        let port_high = (port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = port as u16 as i16;
        let unrolled = self.fresh_label();
        let tail = self.fresh_label();
        let tail_body = self.fresh_label();
        let exit = self.fresh_label();
        self.output.pre_scheduled = true;

        self.evaluate(&Expression::Variable(global.clone()), global_type, 3)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: command,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: port_high,
            });
        self.output.instructions.push(Instruction::LoadHalfwordZero {
            d: 6,
            a: 3,
            offset: left_displacement,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadHalfwordZero {
            d: 3,
            a: 3,
            offset: right_displacement,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 7, a: 6, b: 3 });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 5,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 6,
            a: 5,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 7,
            immediate: 3,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate { a: 3, s: 3, shift: 2 });
        self.emit_branch_conditional_to(4, 1, exit); // ble
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 3,
                shift: 29,
                begin: 3,
                end: 31,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.emit_branch_conditional_to(12, 2, tail); // beq
        self.bind_label(unrolled);
        for _ in 0..8 {
            self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: 5,
                offset: port_low,
            });
        }
        self.emit_branch_conditional_to(16, 0, unrolled); // bdnz
        self.output
            .instructions
            .push(Instruction::AndImmediateRecord {
                a: 3,
                s: 3,
                immediate: 7,
            });
        self.emit_branch_conditional_to(12, 2, exit); // beq
        self.bind_label(tail);
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(tail_body);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: port_low,
        });
        self.emit_branch_conditional_to(16, 0, tail_body); // bdnz
        self.bind_label(exit);
        self.evaluate(&Expression::Variable(global.clone()), global_type, 3)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 3,
            offset: trailing_displacement,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
