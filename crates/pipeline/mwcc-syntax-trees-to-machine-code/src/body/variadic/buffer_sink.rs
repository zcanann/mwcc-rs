//! Variadic diagnostics helpers with a frame-resident formatting buffer.

#[allow(unused_imports)]
use super::super::*;
use super::VariadicBufferFrame;

struct VariadicBufferSink<'a> {
    buffer_bytes: i16,
    sink: &'a str,
    file: &'a [u8],
    line: i16,
}

impl<'a> VariadicBufferSink<'a> {
    /// Recognize the canonical diagnostics helper produced by Pikmin's
    /// `DEFINE_ERROR` family:
    ///
    /// ```c
    /// va_list args;
    /// va_start(args, fmt);
    /// char buffer[N];
    /// vsprintf(buffer, fmt, args);
    /// sink(file, line, buffer);
    /// ```
    ///
    /// The expanded `va_start` is a comma expression ending in
    /// `__builtin_va_info(&args)`. Requiring the complete data-flow shape keeps
    /// this owner independent of helper/function names while preventing a
    /// partial claim of an arbitrary body-bearing variadic function.
    fn recognize(function: &'a Function) -> Option<Self> {
        let (frame, remaining) = VariadicBufferFrame::recognize(function)?;
        let [Statement::Expression(Expression::Call {
                name: formatter,
                arguments: formatter_arguments,
            }),
            Statement::Expression(Expression::Call {
                name: sink,
                arguments: sink_arguments,
            }),
            Statement::Expression(noop)] = remaining
        else {
            return None;
        };
        if formatter != "vsprintf"
            || !matches!(
                formatter_arguments.as_slice(),
                [Expression::Variable(destination),
                    Expression::Variable(format),
                    Expression::Variable(arguments)]
                    if destination == frame.buffer
                        && format == frame.format_parameter
                        && arguments == frame.va_list
            )
        {
            return None;
        }
        let [Expression::StringLiteral(file), line, Expression::Variable(sink_buffer)] =
            sink_arguments.as_slice()
        else {
            return None;
        };
        let line = i16::try_from(constant_value(line)?).ok()?;
        if sink_buffer != frame.buffer
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
        Some(Self {
            buffer_bytes: frame.buffer_bytes,
            sink,
            file,
            line,
        })
    }
}

impl Generator {
    /// Emit a body-bearing variadic diagnostics helper as one frame transaction.
    ///
    /// The legacy linkage-first ABI saves LR through the caller's linkage area,
    /// then lays out the EABI register-save block, formatting buffer, and
    /// 12-byte `va_list` contiguously. The instruction interleave is measured
    /// from GC/1.2.5n's `_Error__FPce`; it overlaps the incoming-register stores
    /// with construction of the two call argument lists.
    pub(crate) fn try_variadic_buffer_sink(
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
        let Some(plan) = VariadicBufferSink::recognize(function) else {
            return Ok(false);
        };

        const REGISTER_SAVE_BYTES: i16 = 100;
        const BUFFER_OFFSET: i16 = 108;
        const VA_LIST_BYTES: i16 = 12;
        let va_list_offset = (BUFFER_OFFSET + plan.buffer_bytes + 3) & !3;
        let frame_size = (va_list_offset + VA_LIST_BYTES + 7) & !7;
        if frame_size <= REGISTER_SAVE_BYTES
            || frame_size.checked_add(8).is_none()
            || frame_size.checked_add(4).is_none()
        {
            return Ok(false);
        }

        self.frame_size = frame_size;
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec!["vsprintf".to_owned(), plan.sink.to_owned()];
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
        // `va_list.gpr = 1; va_list.fpr = 0` occupies the first two bytes.
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 0x100,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: va_list_offset,
        });
        for register in 6..=10 {
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
            immediate: frame_size + 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: va_list_offset + 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: va_list_offset + 8,
        });
        self.record_relocation(RelocationKind::Rel24, "vsprintf");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "vsprintf".to_owned(),
        });

        self.emit_string_literal(plan.file, 3)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: BUFFER_OFFSET,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, plan.line));
        self.record_relocation(RelocationKind::Rel24, plan.sink);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.sink.to_owned(),
        });
        self.emit_epilogue_and_return();
        // The conditional FPR-save block owns one source-level branch pair.
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    fn local(declared_type: Type, name: &str, array_length: Option<u16>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.to_owned(),
            initializer: None,
            is_volatile: false,
            array_length,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn recognizes_structural_variadic_buffer_sink() {
        let function = Function {
            return_type: Type::Void,
            name: "_Error__FPce".to_owned(),
            is_static: true,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(Pointee::UnsignedChar),
                name: "fmt".to_owned(),
            }],
            locals: vec![
                local(
                    Type::Struct {
                        size: 12,
                        align: 4,
                    },
                    "args",
                    None,
                ),
                local(Type::UnsignedChar, "buffer", Some(2048)),
            ],
            statements: vec![
                Statement::Expression(Expression::Comma {
                    left: Box::new(Expression::Cast {
                        target_type: Type::Void,
                        operand: Box::new(Expression::Variable("fmt".to_owned())),
                    }),
                    right: Box::new(Expression::Call {
                        name: "__builtin_va_info".to_owned(),
                        arguments: vec![Expression::AddressOf {
                            operand: Box::new(Expression::Variable("args".to_owned())),
                        }],
                    }),
                }),
                Statement::Expression(Expression::Call {
                    name: "vsprintf".to_owned(),
                    arguments: vec![
                        Expression::Variable("buffer".to_owned()),
                        Expression::Variable("fmt".to_owned()),
                        Expression::Variable("args".to_owned()),
                    ],
                }),
                Statement::Expression(Expression::Call {
                    name: "halt__6SystemFPciPc".to_owned(),
                    arguments: vec![
                        Expression::StringLiteral(b"nlibmath.cpp".to_vec()),
                        Expression::IntegerLiteral(8),
                        Expression::Variable("buffer".to_owned()),
                    ],
                }),
                Statement::Expression(Expression::Cast {
                    target_type: Type::Void,
                    operand: Box::new(Expression::IntegerLiteral(0)),
                }),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let plan = VariadicBufferSink::recognize(&function).expect("canonical helper");
        assert_eq!(plan.buffer_bytes, 2048);
        assert_eq!(plan.sink, "halt__6SystemFPciPc");
        assert_eq!(plan.file, b"nlibmath.cpp");
        assert_eq!(plan.line, 8);
    }
}
