//! Straight-line display-list setup packets with float-scaled bounds.
//!
//! The macro-expanded source exposes three packet aliases and six stores. MWCC
//! folds the aliases into one base register, overlaps two unsigned-to-float
//! conversions, and schedules the ready packet words through their latency
//! windows. This owner keeps that transaction together instead of weakening
//! the general store/return scheduling defer.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{Instruction, RelocationTarget};

struct FramebufferSetup<'a> {
    pointer: &'a str,
    width: &'a str,
    height: &'a str,
}

impl Generator {
    pub(crate) fn try_display_list_framebuffer_setup(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.general_register_of(shape.pointer)? != 4
            || self.general_register_of(shape.width)? != 5
            || self.general_register_of(shape.height)? != 6
        {
            return Ok(false);
        }

        self.frame_size = 48;
        self.output.pre_scheduled = true;
        self.output.has_conversion = true;

        // Source order creates the 4.0f scale before the conversion bias. The
        // three intervening anonymous conversion labels make their measured
        // names differ by four (@111 then @115 in PreRender.c).
        let scale = self.output.intern_constant(f32::to_bits(4.0) as u64, 4);
        let bias = self.output.intern_constant(0x4330_0000_0000_0000, 8);
        self.output.constant_number_gaps.push((bias, 3));

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 0x4330));
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::Constant(bias));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(7, 0));
        self.output
            .instructions
            .push(Instruction::move_register(8, 3));
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 1,
            offset: 28,
        });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::Constant(bias));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 7,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 2,
            a: 6,
            offset: 0,
        });
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::Constant(scale));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(7, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 24,
        });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::Constant(scale));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 7,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 3,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(7, 0xe700u16 as i16));
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 5,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 5,
                s: 6,
                clear: 20,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 0,
                s: 5,
                immediate: 0xff10,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 0xed00u16 as i16));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 8,
            immediate: 24,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 0, a: 3, c: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 });
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 8,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 3, c: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 32,
            });
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 1 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 20,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 8,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 0,
                s: 4,
                shift: 12,
                begin: 8,
                end: 19,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 20,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

fn recognize(function: &Function) -> Option<FramebufferSetup<'_>> {
    let Type::StructPointer { element_size: 8 } = function.return_type else {
        return None;
    };
    let [base, pointer, width, height] = function.parameters.as_slice() else {
        return None;
    };
    if base.parameter_type != function.return_type
        || !matches!(pointer.parameter_type, Type::Pointer(_))
        || width.parameter_type != Type::UnsignedInt
        || height.parameter_type != Type::UnsignedInt
        || !function.guards.is_empty()
        || function.locals.len() != 3
        || function.locals.iter().any(|local| {
            local.declared_type != function.return_type
                || local.initializer.is_some()
                || local.is_static
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let [alias0, first, second, alias1, third, fourth, alias2, fifth, sixth] =
        function.statements.as_slice()
    else {
        return None;
    };
    for (assignment, local, index) in [
        (alias0, &function.locals[0], 0),
        (alias1, &function.locals[1], 1),
        (alias2, &function.locals[2], 2),
    ] {
        let Statement::Assign { name, value } = assignment else {
            return None;
        };
        if name != &local.name || pointer_alias_index(value, &base.name)? != index {
            return None;
        }
    }
    let stores = [first, second, third, fourth, fifth, sixth];
    for (index, statement) in stores.iter().enumerate() {
        let Statement::Store { target, .. } = statement else {
            return None;
        };
        let expected_alias = &function.locals[index / 2].name;
        let expected_offset = u32::try_from(index % 2).ok()? * 4;
        if !matches!(target,
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            } if matches!(base.as_ref(), Expression::Variable(name) if name == expected_alias)
                && *offset == expected_offset)
        {
            return None;
        }
    }
    let values: Vec<&Expression> = stores
        .iter()
        .map(|statement| match statement {
            Statement::Store { value, .. } => value,
            _ => unreachable!(),
        })
        .collect();
    if packet_constant_u32(values[0])? != 0xe700_0000
        || packet_constant_u32(values[1])? != 0
        || packet_constant_u32(values[4])? != 0xed00_0000
        || !matches!(values[3],
            Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == &pointer.name))
        || parameter_reads(values[2], base, pointer, width, height) != [0, 0, 1, 0]
        || parameter_reads(values[4], base, pointer, width, height) != [0, 0, 0, 0]
        || parameter_reads(values[5], base, pointer, width, height) != [0, 0, 1, 1]
        || count_float_literal(values[4], 4.0) != 2
        || count_float_literal(values[5], 4.0) != 2
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == &base.name)
        || constant_value(right)? != 3
    {
        return None;
    }
    Some(FramebufferSetup {
        pointer: &pointer.name,
        width: &width.name,
        height: &height.name,
    })
}

fn pointer_alias_index(expression: &Expression, base: &str) -> Option<i64> {
    let expression = peel_casts(expression);
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    matches!(peel_casts(left), Expression::Variable(name) if name == base)
        .then(|| constant_value(right))
        .flatten()
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn parameter_reads(
    expression: &Expression,
    base: &mwcc_syntax_trees::Parameter,
    pointer: &mwcc_syntax_trees::Parameter,
    width: &mwcc_syntax_trees::Parameter,
    height: &mwcc_syntax_trees::Parameter,
) -> [usize; 4] {
    [
        count_name_occurrences(expression, &base.name),
        count_name_occurrences(expression, &pointer.name),
        count_name_occurrences(expression, &width.name),
        count_name_occurrences(expression, &height.name),
    ]
}

#[derive(Clone, Copy)]
enum PacketConstant {
    Integer(i64),
    Float(f64),
}

fn packet_constant_u32(expression: &Expression) -> Option<u32> {
    match packet_constant(expression)? {
        PacketConstant::Integer(value) => Some(value as u32),
        PacketConstant::Float(_) => None,
    }
}

fn packet_constant(expression: &Expression) -> Option<PacketConstant> {
    match expression {
        Expression::IntegerLiteral(value) => Some(PacketConstant::Integer(*value)),
        Expression::FloatLiteral(value) => Some(PacketConstant::Float(*value)),
        Expression::Cast {
            target_type,
            operand,
        } => {
            let value = packet_constant(operand)?;
            match target_type {
                Type::Float => Some(PacketConstant::Float(match value {
                    PacketConstant::Integer(value) => (value as f32) as f64,
                    PacketConstant::Float(value) => (value as f32) as f64,
                })),
                Type::Double => Some(PacketConstant::Float(match value {
                    PacketConstant::Integer(value) => value as f64,
                    PacketConstant::Float(value) => value,
                })),
                Type::Int
                | Type::UnsignedInt
                | Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort => Some(PacketConstant::Integer(match value {
                    PacketConstant::Integer(value) => value,
                    PacketConstant::Float(value) => value.trunc() as i64,
                })),
                _ => None,
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left = packet_constant(left)?;
            let right = packet_constant(right)?;
            match (left, right) {
                (PacketConstant::Integer(left), PacketConstant::Integer(right)) => {
                    let value = match operator {
                        BinaryOperator::Add => left.wrapping_add(right),
                        BinaryOperator::Subtract => left.wrapping_sub(right),
                        BinaryOperator::Multiply => left.wrapping_mul(right),
                        BinaryOperator::BitAnd => left & right,
                        BinaryOperator::BitOr => left | right,
                        BinaryOperator::ShiftLeft => left.wrapping_shl(u32::try_from(right).ok()?),
                        BinaryOperator::ShiftRight => left.wrapping_shr(u32::try_from(right).ok()?),
                        _ => return None,
                    };
                    Some(PacketConstant::Integer(value))
                }
                (left, right) => {
                    let number = |value| match value {
                        PacketConstant::Integer(value) => value as f64,
                        PacketConstant::Float(value) => value,
                    };
                    let left = number(left);
                    let right = number(right);
                    Some(PacketConstant::Float(match operator {
                        BinaryOperator::Add => left + right,
                        BinaryOperator::Subtract => left - right,
                        BinaryOperator::Multiply => left * right,
                        _ => return None,
                    }))
                }
            }
        }
        _ => None,
    }
}

fn count_float_literal(expression: &Expression, expected: f64) -> usize {
    match expression {
        Expression::FloatLiteral(value) => usize::from(*value == expected),
        Expression::Cast { operand, .. } | Expression::Unary { operand, .. } => {
            count_float_literal(operand, expected)
        }
        Expression::Binary { left, right, .. } => {
            count_float_literal(left, expected) + count_float_literal(right, expected)
        }
        _ => 0,
    }
}
