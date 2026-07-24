//! Variadic formatting helpers guarded by a global output stream.

#[allow(unused_imports)]
use super::super::*;
use super::VariadicBufferFrame;

struct VariadicBufferPrint<'a> {
    buffer_bytes: i16,
    stream: &'a str,
    prefix_callee: &'a str,
    prefix_format: &'a [u8],
    prefix_name: &'a [u8],
    formatter: &'a str,
    length: &'a str,
    vptr_offset: u16,
    slot_offset: u16,
}

impl<'a> VariadicBufferPrint<'a> {
    /// Recognize a guarded stream helper without depending on its source names:
    ///
    /// ```c
    /// va_list args;
    /// char buffer[N];
    /// va_start(args, fmt);
    /// if (stream) {
    ///     stream->print("%s: ", component);
    ///     vsprintf(buffer, fmt, args);
    ///     if (strlen(buffer))
    ///         stream->write(buffer, strlen(buffer));
    /// }
    /// ```
    ///
    /// Every use is tied back to the recovered frame/global names, including
    /// the repeated length call and virtual-call receiver. This owner therefore
    /// cannot partially claim a more general variadic body.
    fn recognize(function: &'a Function) -> Option<Self> {
        let (frame, remaining) = VariadicBufferFrame::recognize(function)?;
        let [
            Statement::If {
                condition: Expression::Variable(stream),
                then_body,
                else_body,
            },
            Statement::Expression(noop),
        ] = remaining
        else {
            return None;
        };
        if !else_body.is_empty()
            || !matches!(
                noop,
                Expression::Cast {
                    target_type: Type::Void,
                    operand,
                } if constant_value(operand) == Some(0)
            )
        {
            return None;
        }
        let [
            Statement::If {
                condition: Expression::StringLiteral(prefix_name),
                then_body: prefix_body,
                else_body: prefix_else,
            },
            Statement::Expression(Expression::Call {
                name: formatter,
                arguments: formatter_arguments,
            }),
            Statement::If {
                condition:
                    Expression::Call {
                        name: condition_length,
                        arguments: condition_arguments,
                    },
                then_body: write_body,
                else_body: write_else,
            },
        ] = then_body.as_slice()
        else {
            return None;
        };
        let [
            Statement::Expression(Expression::Call {
                name: prefix_callee,
                arguments: prefix_arguments,
            }),
        ] = prefix_body.as_slice()
        else {
            return None;
        };
        let [
            Expression::Variable(prefix_stream),
            Expression::StringLiteral(prefix_format),
            Expression::StringLiteral(prefix_argument),
        ] = prefix_arguments.as_slice()
        else {
            return None;
        };
        if !prefix_else.is_empty()
            || prefix_stream != stream
            || prefix_argument.as_slice() != prefix_name.as_slice()
            || !matches!(
                formatter_arguments.as_slice(),
                [Expression::Variable(destination),
                    Expression::Variable(format),
                    Expression::Variable(arguments)]
                    if destination == frame.buffer
                        && format == frame.format_parameter
                        && arguments == frame.va_list
            )
            || !matches!(
                condition_arguments.as_slice(),
                [Expression::Variable(buffer)] if buffer == frame.buffer
            )
            || !write_else.is_empty()
        {
            return None;
        }
        let [
            Statement::Expression(Expression::VirtualCall {
                object,
                vptr_offset,
                slot_offset,
                return_type: Type::Void,
                variadic: false,
                arguments: write_arguments,
            }),
        ] = write_body.as_slice()
        else {
            return None;
        };
        let [
            Expression::Variable(write_buffer),
            Expression::Call {
                name: write_length,
                arguments: write_length_arguments,
            },
        ] = write_arguments.as_slice()
        else {
            return None;
        };
        if !matches!(
            object.as_ref(),
            Expression::Variable(write_stream) if write_stream == stream
        ) || write_buffer != frame.buffer
            || write_length != condition_length
            || !matches!(
                write_length_arguments.as_slice(),
                [Expression::Variable(buffer)] if buffer == frame.buffer
            )
        {
            return None;
        }
        Some(Self {
            buffer_bytes: frame.buffer_bytes,
            stream,
            prefix_callee,
            prefix_format,
            prefix_name,
            formatter,
            length: condition_length,
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
        })
    }
}

impl Generator {
    /// Emit the linkage-first frame and measured GC/1.2.5n instruction schedule
    /// for a guarded variadic stream helper.
    pub(crate) fn try_variadic_buffer_print(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.variadic_definition
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.entry_parameter_words != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(plan) = VariadicBufferPrint::recognize(function) else {
            return Ok(false);
        };

        const BUFFER_OFFSET: i16 = 108;
        const VA_LIST_BYTES: i16 = 12;
        const CALLEE_SAVE_BYTES: i16 = 8;
        let va_list_offset = (BUFFER_OFFSET + plan.buffer_bytes + 3) & !3;
        let frame_size =
            (va_list_offset + VA_LIST_BYTES + CALLEE_SAVE_BYTES + 7) & !7;
        if frame_size <= 108
            || frame_size.checked_add(8).is_none()
            || plan.vptr_offset > (i16::MAX - 4) as u16
            || plan.slot_offset > i16::MAX as u16
        {
            return Ok(false);
        }

        self.frame_size = frame_size;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30];
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![
            plan.stream.to_owned(),
            plan.prefix_callee.to_owned(),
            plan.formatter.to_owned(),
            plan.length.to_owned(),
        ];

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
                offset: -frame_size,
            });
        for (index, &register) in self.callee_saved.iter().enumerate() {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset: frame_size - 4 * (index as i16 + 1),
            });
        }

        let skip_float_saves = self.fresh_label();
        self.emit_branch_conditional_to(4, 6, skip_float_saves);
        for register in 1..=8 {
            self.output.instructions.push(Instruction::StoreFloatDouble {
                s: register,
                a: 1,
                offset: 32 + i16::from(register) * 8,
            });
        }
        self.bind_label(skip_float_saves);

        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 0x100,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: frame_size + 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 1,
            immediate: va_list_offset,
        });
        for register in 5..=10 {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset: -4 + i16::from(register) * 4,
            });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: va_list_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: va_list_offset + 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: va_list_offset + 8,
        });

        self.emit_global_load_value(plan.stream, 3)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done);

        // Intern in source argument order so the unit-wide anonymous-symbol
        // resolver preserves MWCC's `%s: ` before component-name ordering.
        let _prefix_format = self.string_literal_placeholder(plan.prefix_format);
        let prefix_name = self.string_literal_placeholder(plan.prefix_name);
        self.emit_address_high(4, &prefix_name);
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.emit_string_address_low(&prefix_name, 4, 5);
        self.emit_string_literal(plan.prefix_format, 4)?;
        self.record_relocation(RelocationKind::Rel24, plan.prefix_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.prefix_callee.to_owned(),
        });

        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.record_relocation(RelocationKind::Rel24, plan.formatter);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.formatter.to_owned(),
        });

        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.record_relocation(RelocationKind::Rel24, plan.length);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.length.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);

        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.record_relocation(RelocationKind::Rel24, plan.length);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.length.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 3));
        self.emit_global_load_value(plan.stream, 3)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            // MWCC's primary-object ABI keeps the vptr after the leading
            // runtime/type word; the AST offset is relative to that vptr slot.
            offset: plan.vptr_offset as i16 + 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 12,
            offset: plan.slot_offset as i16,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegisterAndLink);

        self.bind_label(done);
        self.emit_epilogue_and_return();
        // FPR-save control flow and the two source `if` statements each own an
        // anonymous label pair in this legacy frontend.
        self.output.anonymous_label_bump += 6;
        Ok(true)
    }
}
