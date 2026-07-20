//! A guarded dirty-bit dispatcher followed by a flush test and fixed-width FIFO writes.

#[allow(unused_imports)]
use super::*;

use super::bitmask_call_chain::recognize_bit_calls;

fn zero_call(statement: &Statement) -> Option<&str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    arguments.is_empty().then_some(name)
}

fn fixed_store<'a>(statement: &'a Statement, width: Type) -> Option<(&'a Expression, u32)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let operand = match target {
        Expression::Member {
            base,
            offset: 0,
            member_type,
            index_stride: None,
        } if *member_type == width => {
            let Expression::Cast {
                target_type: Type::StructPointer { .. },
                operand,
            } = base.as_ref()
            else {
                return None;
            };
            operand.as_ref()
        }
        Expression::Dereference { pointer } => {
            let expected = match width {
                Type::UnsignedChar => Pointee::UnsignedChar,
                Type::UnsignedShort => Pointee::UnsignedShort,
                _ => return None,
            };
            let Expression::Cast {
                target_type: Type::Pointer(pointee),
                operand,
            } = pointer.as_ref()
            else {
                return None;
            };
            if *pointee != expected {
                return None;
            }
            operand.as_ref()
        }
        _ => return None,
    };
    let address = constant_value(operand).and_then(|value| u32::try_from(value).ok())?;
    Some((value, address))
}

impl Generator {
    /// Lower the older Dolphin SDK `GXBegin` family without keying on its spelling. The shape is a
    /// guarded dirty-word call chain, a global-word zero test with one flush call, then byte and
    /// halfword writes to the same fixed port. Its three incoming values and dirty word all cross
    /// calls, giving the measured r28..r31 linkage-first frame.
    pub(crate) fn try_guarded_bitmask_call_sequence(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return Ok(false);
        }

        let [type_parameter, format_parameter, count_parameter] = function.parameters.as_slice()
        else {
            return Ok(false);
        };
        if type_parameter.parameter_type != Type::Int
            || format_parameter.parameter_type != Type::Int
            || count_parameter.parameter_type != Type::UnsignedShort
        {
            return Ok(false);
        }
        let [data_local, mask_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        let Type::StructPointer { .. } = data_local.declared_type else {
            return Ok(false);
        };
        let Some(Expression::Variable(global)) = data_local.initializer.as_ref() else {
            return Ok(false);
        };
        if data_local.array_length.is_some()
            || data_local.is_static
            || mask_local.declared_type != Type::UnsignedInt
            || mask_local.array_length.is_some()
            || mask_local.is_static
        {
            return Ok(false);
        }
        let Some(Expression::Member {
            base: mask_base,
            offset: mask_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }) = mask_local.initializer.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(mask_base.as_ref(), Expression::Variable(name) if name == &data_local.name) {
            return Ok(false);
        }
        let Ok(mask_displacement) = i16::try_from(*mask_offset) else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };

        let [outer, flush, byte_store, half_store] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: outer_left,
                    right: outer_right,
                },
            then_body: outer_body,
            else_body: outer_else,
        } = outer
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: outer_base,
            offset: outer_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } = outer_left.as_ref()
        else {
            return Ok(false);
        };
        if constant_value(outer_right) != Some(0)
            || *outer_offset != *mask_offset
            || !matches!(outer_base.as_ref(), Expression::Variable(name) if name == &data_local.name)
            || !outer_else.is_empty()
        {
            return Ok(false);
        }
        let Some((clear, bit_statements)) = outer_body.split_last() else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: clear_base,
                    offset: clear_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value: clear_value,
        } = clear
        else {
            return Ok(false);
        };
        if constant_value(clear_value) != Some(0)
            || *clear_offset != *mask_offset
            || !matches!(clear_base.as_ref(), Expression::Variable(name) if name == global)
        {
            return Ok(false);
        }
        let Some(bit_calls) = recognize_bit_calls(bit_statements, &mask_local.name) else {
            return Ok(false);
        };

        let Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: flush_left,
                    right: flush_right,
                },
            then_body: flush_body,
            else_body: flush_else,
        } = flush
        else {
            return Ok(false);
        };
        let Expression::Dereference { pointer } = flush_left.as_ref() else {
            return Ok(false);
        };
        let Expression::Cast {
            target_type: Type::Pointer(Pointee::UnsignedInt),
            operand: flush_global,
        } = pointer.as_ref()
        else {
            return Ok(false);
        };
        let [flush_statement] = flush_body.as_slice() else {
            return Ok(false);
        };
        let Some(flush_callee) = zero_call(flush_statement) else {
            return Ok(false);
        };
        if constant_value(flush_right) != Some(0)
            || !matches!(flush_global.as_ref(), Expression::Variable(name) if name == global)
            || !flush_else.is_empty()
        {
            return Ok(false);
        }

        let Some((byte_value, byte_address)) = fixed_store(byte_store, Type::UnsignedChar) else {
            return Ok(false);
        };
        let Some((half_value, half_address)) = fixed_store(half_store, Type::UnsignedShort) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: byte_left,
            right: byte_right,
        } = byte_value
        else {
            return Ok(false);
        };
        if byte_address != half_address
            || byte_address & 0xffff != 0x8000
            || !matches!(byte_left.as_ref(), Expression::Variable(name) if name == &format_parameter.name)
            || !matches!(byte_right.as_ref(), Expression::Variable(name) if name == &type_parameter.name)
            || !matches!(half_value, Expression::Variable(name) if name == &count_parameter.name)
        {
            return Ok(false);
        }

        const SAVED_MASK: u8 = 31;
        const SAVED_COUNT: u8 = 30;
        const SAVED_FORMAT: u8 = 29;
        const SAVED_TYPE: u8 = 28;
        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![SAVED_MASK, SAVED_COUNT, SAVED_FORMAT, SAVED_TYPE];
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump = 5 + 2 * bit_calls.len() as u32;

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED_MASK,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED_COUNT,
            a: 1,
            offset: 32,
        });
        self.emit_callee_saved_home_copy(SAVED_COUNT, 5);
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED_FORMAT,
            a: 1,
            offset: 28,
        });
        self.emit_callee_saved_home_copy(SAVED_FORMAT, 4);
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED_TYPE,
            a: 1,
            offset: 24,
        });
        self.emit_callee_saved_home_copy(SAVED_TYPE, 3);

        self.evaluate(&Expression::Variable(global.clone()), global_type, 6)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: SAVED_MASK,
            a: 6,
            offset: mask_displacement,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: SAVED_MASK,
                immediate: 0,
            });
        let skip_dirty = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.emit_saved_bit_calls(&bit_calls, SAVED_MASK);
        self.emit_store(
            match clear {
                Statement::Store { target, .. } => target,
                _ => unreachable!(),
            },
            clear_value,
        )?;
        let after_dirty = self.output.instructions.len();
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[skip_dirty]
        else {
            unreachable!()
        };
        *target = after_dirty;

        self.evaluate(&Expression::Variable(global.clone()), global_type, 3)?;
        self.output
            .instructions
            .push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        let skip_flush = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.record_relocation(RelocationKind::Rel24, flush_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: flush_callee.to_string(),
        });
        let after_flush = self.output.instructions.len();
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[skip_flush]
        else {
            unreachable!()
        };
        *target = after_flush;

        self.output.instructions.push(Instruction::Or {
            a: 0,
            s: SAVED_FORMAT,
            b: SAVED_TYPE,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: (byte_address.wrapping_add(0x8000) >> 16) as u16 as i16,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 3,
            offset: byte_address as u16 as i16,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: SAVED_COUNT,
            a: 3,
            offset: half_address as u16 as i16,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
