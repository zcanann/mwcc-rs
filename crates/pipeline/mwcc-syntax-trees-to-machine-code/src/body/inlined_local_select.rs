//! O0 callers that consume an expanded local-select helper.
//!
//! The callee body is classified once by `inline_summaries`; this module owns
//! only the caller-side allocation and copy chain. That keeps interprocedural
//! recognition separate from the schedule it enables.

#[allow(unused_imports)]
use super::*;

use mwcc_syntax_trees::Parameter;
use mwcc_versions::Optimization;

struct InlinedBitTest<'a> {
    parameter: &'a Parameter,
    pointer: &'a LocalDeclaration,
    index: &'a LocalDeclaration,
    helper: &'a str,
}

fn variable_is(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Variable(name) => name == expected,
        Expression::Cast { operand, .. } => variable_is(operand, expected),
        _ => false,
    }
}

fn binary_constant<'a>(
    expression: &'a Expression,
    operator: BinaryOperator,
    constant: i64,
) -> Option<&'a Expression> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*actual == operator && constant_value(right) == Some(constant)).then_some(left)
}

fn is_canonical_bit_test(expression: &Expression, pointer: &str, index: &str) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return false;
    };
    let Expression::Index {
        base,
        index: byte_index,
    } = left.as_ref()
    else {
        return false;
    };
    if !variable_is(base, pointer)
        || !binary_constant(byte_index, BinaryOperator::Divide, 8)
            .is_some_and(|value| variable_is(value, index))
    {
        return false;
    }
    // `binary_constant` is unsuitable for this outer shift because its
    // constant is the left operand in `1 << (index % 8)`.
    matches!(right.as_ref(), Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: one,
        right: amount,
    } if constant_value(one) == Some(1)
        && binary_constant(amount, BinaryOperator::Modulo, 8)
            .is_some_and(|value| variable_is(value, index)))
}

fn classify(function: &Function) -> Option<InlinedBitTest<'_>> {
    if !function.statements.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
    {
        return None;
    }
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [pointer, index] = function.locals.as_slice() else {
        return None;
    };
    if parameter.parameter_type != Type::UnsignedInt
        || pointer.declared_type != Type::Pointer(Pointee::UnsignedChar)
        || index.declared_type != Type::UnsignedShort
        || pointer.is_static
        || index.is_static
        || pointer.is_volatile
        || index.is_volatile
        || pointer.array_length.is_some()
        || index.array_length.is_some()
    {
        return None;
    }
    let Expression::Call { name, arguments } = pointer.initializer.as_ref()? else {
        return None;
    };
    let [argument] = arguments.as_slice() else {
        return None;
    };
    if !variable_is(argument, &parameter.name)
        || !index
            .initializer
            .as_ref()
            .is_some_and(|value| variable_is(value, &parameter.name))
    {
        return None;
    }
    let return_expression = function.return_expression.as_ref()?;
    if !is_canonical_bit_test(return_expression, &pointer.name, &index.name) {
        return None;
    }
    Some(InlinedBitTest {
        parameter,
        pointer,
        index,
        helper: name,
    })
}

impl Generator {
    /// Emit the promoted `u16` spelling of
    /// `bytes[index / 8] & (1 << (index % 8))` at O0. Although the source index
    /// is non-negative, C promotion makes both divide and modulo signed; MWCC
    /// therefore preserves its signed power-of-two correction sequences.
    fn emit_unoptimized_indexed_bit_test(&mut self, pointer: u8, index: u8) {
        const ONE: u8 = 5;
        const VALUE: u8 = 4;

        self.output
            .instructions
            .push(Instruction::load_immediate(ONE, 1));
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: VALUE,
                s: index,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: VALUE,
                shift: 29,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: VALUE,
                s: VALUE,
                shift: 31,
            });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: GENERAL_SCRATCH,
            a: VALUE,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: GENERAL_SCRATCH,
            s: GENERAL_SCRATCH,
            shift: 3,
            begin: 0,
            end: 31,
        });
        self.output.instructions.push(Instruction::Add {
            d: GENERAL_SCRATCH,
            a: GENERAL_SCRATCH,
            b: VALUE,
        });
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: VALUE,
            s: ONE,
            b: GENERAL_SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: GENERAL_SCRATCH,
                s: index,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                shift: 3,
            });
        self.output
            .instructions
            .push(Instruction::AddToZeroExtended {
                d: GENERAL_SCRATCH,
                a: GENERAL_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed {
                d: GENERAL_SCRATCH,
                a: pointer,
                b: GENERAL_SCRATCH,
            });
        self.output.instructions.push(Instruction::And {
            a: Eabi::general_result().number,
            s: VALUE,
            b: GENERAL_SCRATCH,
        });
    }

    /// Expand a verified static local-select helper into the O0 caller that
    /// narrows the same input to a byte index and returns one selected bit.
    pub(crate) fn try_inlined_local_select_bit_test(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.optimization != Optimization::O0 {
            return Ok(false);
        }
        let Some(call) = classify(function) else {
            return Ok(false);
        };
        let Some(helper) = self
            .inline_summaries
            .unoptimized_local_select(call.helper)
            .cloned()
        else {
            return Ok(false);
        };
        let [helper_parameter] = helper.parameters.as_slice() else {
            return Ok(false);
        };
        if helper_parameter.parameter_type != call.parameter.parameter_type
            || helper.result_type != call.pointer.declared_type
        {
            return Ok(false);
        }

        const HELPER_DERIVED: u8 = 27;
        const INLINE_RESULT_COPY: u8 = 28;
        const HELPER_RESULT: u8 = 29;
        const INDEX_HOME: u8 = 30;
        const POINTER_HOME: u8 = 31;
        const FIRST_SAVED: u8 = HELPER_DERIVED;

        let parameter_home = self
            .lookup_general(&call.parameter.name)
            .ok_or_else(|| Diagnostic::error("inlined local-select parameter has no GPR home"))?;
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump += 2;
        self.reserved.extend(FIRST_SAVED..=POINTER_HOME);
        self.reserved.insert(parameter_home);
        self.bind_unoptimized_local_select_value(
            &helper_parameter.name,
            helper_parameter.parameter_type,
            parameter_home,
        );

        self.emit_savegpr_frame_prologue(FIRST_SAVED, 32);
        self.emit_unoptimized_local_select_core(&helper, HELPER_RESULT, HELPER_DERIVED)?;
        self.output.instructions.push(Instruction::move_register(
            INLINE_RESULT_COPY,
            HELPER_RESULT,
        ));
        self.output
            .instructions
            .push(Instruction::move_register(POINTER_HOME, INLINE_RESULT_COPY));
        self.bind_unoptimized_local_select_value(
            &call.pointer.name,
            call.pointer.declared_type,
            POINTER_HOME,
        );

        // The u32-to-u16 source local is materialized even though every later
        // use promotes it again; O0 preserves both operations.
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: INDEX_HOME,
                s: parameter_home,
                clear: 16,
            });
        self.bind_unoptimized_local_select_value(
            &call.index.name,
            call.index.declared_type,
            INDEX_HOME,
        );

        self.emit_unoptimized_indexed_bit_test(POINTER_HOME, INDEX_HOME);
        self.emit_restgpr_frame_epilogue(FIRST_SAVED);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(declared_type: Type, name: &str, initializer: Expression) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: Some(initializer),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn recognizes_the_call_narrow_index_bit_test_without_function_names() {
        let function = Function {
            return_type: Type::Int,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::UnsignedInt,
                name: "input".into(),
            }],
            locals: vec![
                local(
                    Type::Pointer(Pointee::UnsignedChar),
                    "bytes",
                    Expression::Call {
                        name: "selector".into(),
                        arguments: vec![Expression::Variable("input".into())],
                    },
                ),
                local(
                    Type::UnsignedShort,
                    "slot",
                    Expression::Variable("input".into()),
                ),
            ],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Index {
                    base: Box::new(Expression::Variable("bytes".into())),
                    index: Box::new(Expression::Binary {
                        operator: BinaryOperator::Divide,
                        left: Box::new(Expression::Variable("slot".into())),
                        right: Box::new(Expression::IntegerLiteral(8)),
                    }),
                }),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::ShiftLeft,
                    left: Box::new(Expression::IntegerLiteral(1)),
                    right: Box::new(Expression::Binary {
                        operator: BinaryOperator::Modulo,
                        left: Box::new(Expression::Variable("slot".into())),
                        right: Box::new(Expression::IntegerLiteral(8)),
                    }),
                }),
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let shape = classify(&function).expect("recognized");
        assert_eq!(shape.helper, "selector");
        assert_eq!(shape.pointer.name, "bytes");
        assert_eq!(shape.index.name, "slot");
    }
}
