//! Variadic formatting helpers guarded by a global output stream.

#[allow(unused_imports)]
use super::super::*;
use super::VariadicBufferFrame;

struct VariadicBufferPrint<'a> {
    buffer_bytes: i16,
    stream: &'a str,
    prefix: Option<VariadicPrefix<'a>>,
    formatter: &'a str,
    length: &'a str,
    vptr_offset: u16,
    slot_offset: u16,
}

struct VariadicPrefix<'a> {
    callee: &'a str,
    format: &'a [u8],
    name: &'a [u8],
}

fn complete_object_vptr_offset(offset: u16) -> Option<i16> {
    i16::try_from(offset).ok()
}

fn recognize_prefix<'a>(
    statement: &'a Statement,
    stream: &str,
) -> Option<Option<VariadicPrefix<'a>>> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let [
        Statement::Expression(Expression::Call {
            name: callee,
            arguments,
        }),
    ] = then_body.as_slice()
    else {
        return None;
    };
    let [
        Expression::Variable(prefix_stream),
        Expression::StringLiteral(format),
        argument,
    ] = arguments.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() || prefix_stream != stream {
        return None;
    }
    match condition {
        Expression::StringLiteral(name)
            if matches!(
                argument,
                Expression::StringLiteral(argument) if argument == name
            ) =>
        {
            Some(Some(VariadicPrefix {
                callee,
                format,
                name,
            }))
        }
        condition
            if constant_value(condition) == Some(0)
                && constant_value(argument) == Some(0) =>
        {
            Some(None)
        }
        _ => None,
    }
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
            prefix_statement,
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
        let prefix = recognize_prefix(prefix_statement, stream)?;
        if !matches!(
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
            prefix,
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
        let Some(vptr_offset) = complete_object_vptr_offset(plan.vptr_offset) else {
            return Ok(false);
        };

        const BUFFER_OFFSET: i16 = 108;
        const VA_LIST_BYTES: i16 = 12;
        let has_prefix = plan.prefix.is_some();
        let callee_save_bytes = if has_prefix { 8 } else { 0 };
        let va_list_offset = (BUFFER_OFFSET + plan.buffer_bytes + 3) & !3;
        let frame_size = (va_list_offset + VA_LIST_BYTES + callee_save_bytes + 7) & !7;
        if frame_size <= 108
            || frame_size.checked_add(8).is_none()
            || plan.slot_offset > i16::MAX as u16
        {
            return Ok(false);
        }

        self.frame_size = frame_size;
        self.non_leaf = true;
        self.callee_saved = if has_prefix { vec![31, 30] } else { Vec::new() };
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![plan.stream.to_owned()];
        if let Some(prefix) = &plan.prefix {
            self.output.symbol_order.push(prefix.callee.to_owned());
        }
        self.output
            .symbol_order
            .extend([plan.formatter.to_owned(), plan.length.to_owned()]);

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
        if has_prefix {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 30,
                a: 3,
                immediate: 0,
            });
        }
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
        if has_prefix {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 31,
                a: 1,
                immediate: va_list_offset,
            });
        }
        for register in 5..=10 {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset: -4 + i16::from(register) * 4,
            });
            if register == 5 && !has_prefix {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 5,
                    a: 1,
                    immediate: va_list_offset,
                });
            }
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

        let stream_register = if has_prefix { 3 } else { 0 };
        self.emit_global_load_value(plan.stream, stream_register)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: stream_register,
                immediate: 0,
            });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done);

        if let Some(prefix) = &plan.prefix {
            // Intern in source argument order so the unit-wide anonymous-symbol
            // resolver preserves MWCC's `%s: ` before component-name ordering.
            let _prefix_format = self.string_literal_placeholder(prefix.format);
            let prefix_name = self.string_literal_placeholder(prefix.name);
            self.emit_address_high(4, &prefix_name);
            self.output
                .instructions
                .push(Instruction::ConditionRegisterClear { d: 6 });
            self.emit_string_address_low(&prefix_name, 4, 5);
            self.emit_string_literal(prefix.format, 4)?;
            self.record_relocation(RelocationKind::Rel24, prefix.callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: prefix.callee.to_owned(),
            });
        }

        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: if has_prefix { 30 } else { 3 },
            immediate: 0,
        });
        if has_prefix {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 5,
                a: 31,
                immediate: 0,
            });
        }
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
            // Class layout recovery records the complete-object byte offset.
            // A leading data word is therefore already reflected here (for
            // example Stream::mPath at 0 followed by its vptr at 4).
            offset: vptr_offset,
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
        // The measured GC/1.2.5n analysis walk consumes seven ordinals across
        // the variadic-save and guarded-print control flow.
        self.output.anonymous_label_bump += 7;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_prefix_is_a_proven_dead_optional_transaction() {
        let statement = Statement::If {
            condition: Expression::IntegerLiteral(0),
            then_body: vec![Statement::Expression(Expression::Call {
                name: "print".into(),
                arguments: vec![
                    Expression::Variable("stream".into()),
                    Expression::StringLiteral(b"%s: ".to_vec()),
                    Expression::IntegerLiteral(0),
                ],
            })],
            else_body: Vec::new(),
        };

        assert!(matches!(
            recognize_prefix(&statement, "stream"),
            Some(None)
        ));
    }

    #[test]
    fn recovered_vptr_offsets_are_complete_object_offsets() {
        assert_eq!(complete_object_vptr_offset(4), Some(4));
        assert_eq!(complete_object_vptr_offset(u16::MAX), None);
    }
}
