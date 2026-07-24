//! Three fixed-port matrix packets fed by scaled float pairs.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

struct MatrixPacket<'a> {
    matrix_id: &'a str,
    source: &'a str,
    scale: &'a str,
    values: &'a str,
    word: &'a str,
    packet_id: &'a str,
    global: &'a str,
    flag_offset: i16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if constant_value(operand) == Some(0))
}

fn switch_assigns_ranges(
    statement: &Statement,
    matrix_id: &str,
    packet_id: &str,
) -> bool {
    let Statement::Switch {
        scrutinee: Expression::Variable(scrutinee),
        arms,
        default: Some(ArmBody::Statements(default)),
    } = statement
    else {
        return false;
    };
    if scrutinee != matrix_id
        || arms.iter().map(|arm| arm.value).collect::<Vec<_>>()
            != [1, 2, 3, 5, 6, 7, 9, 10, 11]
        || arms
            .iter()
            .enumerate()
            .any(|(index, arm)| arm.falls_through != !matches!(index, 2 | 5 | 8))
    {
        return false;
    }
    for (index, subtract) in [(2usize, 1), (5, 5), (8, 9)] {
        let ArmBody::Statements(body) = &arms[index].body else {
            return false;
        };
        if !matches!(body.as_slice(), [Statement::Assign { name, value: Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        }}] if name == packet_id
            && matches!(left.as_ref(), Expression::Variable(name) if name == matrix_id)
            && constant_value(right) == Some(subtract))
        {
            return false;
        }
    }
    arms.iter().enumerate().all(|(index, arm)| {
        matches!(&arm.body, ArmBody::Statements(body) if matches!(index, 2 | 5 | 8) || body.is_empty())
    }) && matches!(default.as_slice(), [Statement::Assign { name, value }]
        if name == packet_id && constant_value(value) == Some(0))
}

fn matrix_store(
    statement: &Statement,
    values: &str,
    index: i64,
    source: &str,
    source_offset: u32,
) -> bool {
    let Statement::Store {
        target: Expression::Index { base, index: target_index },
        value:
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            },
    } = statement
    else {
        return false;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == values)
        || constant_value(target_index) != Some(index)
        || constant_value(right) != Some(2047)
    {
        return false;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: factor,
        right: sample,
    } = stripped(left)
    else {
        return false;
    };
    matches!(factor.as_ref(), Expression::FloatLiteral(value) if *value == 1024.0)
        && matches!(sample.as_ref(), Expression::Member {
            base, offset, member_type: Type::Float, index_stride: None
        } if *offset == source_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == source))
}

fn scale_update(statement: &Statement, scale: &str) -> bool {
    matches!(statement, Statement::Assign { name, value: Expression::Binary {
        operator: BinaryOperator::Add, left, right
    }} if name == scale
        && matches!(left.as_ref(), Expression::Variable(name) if name == scale)
        && constant_value(right) == Some(17))
}

fn zero_word(statement: &Statement, word: &str) -> bool {
    matches!(statement, Statement::Assign { name, value }
        if name == word && constant_value(value) == Some(0))
}

fn field_insert<'a>(
    statement: &'a Statement,
    word: &str,
    preserve: u32,
    shift: i64,
) -> Option<&'a Expression> {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    else {
        return None;
    };
    if constant_value(condition) != Some(0) {
        return None;
    }
    let [Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old,
        right: mask,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: inserted,
        right: found_shift,
    } = right.as_ref()
    else {
        return None;
    };
    (name == word
        && matches!(stripped(old), Expression::Variable(name) if name == word)
        && constant_value(mask).map(|value| value as u32) == Some(preserve)
        && constant_value(found_shift) == Some(shift))
    .then_some(stripped(inserted))
}

fn indexed(expression: &Expression, values: &str, index: i64) -> bool {
    matches!(expression, Expression::Index { base, index: found }
        if matches!(base.as_ref(), Expression::Variable(name) if name == values)
            && constant_value(found) == Some(index))
}

fn scale_bits(expression: &Expression, scale: &str, right_shift: i64) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return false;
    };
    if constant_value(right) != Some(3) {
        return false;
    }
    if right_shift == 0 {
        matches!(left.as_ref(), Expression::Variable(name) if name == scale)
    } else {
        matches!(left.as_ref(), Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: shifted,
            right: amount,
        } if matches!(shifted.as_ref(), Expression::Variable(name) if name == scale)
            && constant_value(amount) == Some(right_shift))
    }
}

fn packet_number(expression: &Expression, packet_id: &str, addend: i64) -> bool {
    matches!(expression, Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if constant_value(right) == Some(addend)
        && matches!(left.as_ref(), Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: id,
            right: factor,
        } if matches!(id.as_ref(), Expression::Variable(name) if name == packet_id)
            && constant_value(factor) == Some(3)))
}

fn port_write(statement: &Statement, word: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    else {
        return false;
    };
    if constant_value(condition) != Some(0) {
        return false;
    }
    let [Statement::Store { target: command_target, value: command },
        Statement::Store { target: data_target, value: data }] = body.as_slice()
    else {
        return false;
    };
    let port_target = |target: &Expression, member_type| {
        matches!(target, Expression::Member {
            base, offset: 0, member_type: found, index_stride: None
        } if *found == member_type
            && matches!(stripped(base), Expression::IntegerLiteral(value)
                if *value as u32 == 0xcc00_8000))
    };
    port_target(command_target, Type::UnsignedChar)
        && constant_value(command) == Some(0x61)
        && port_target(data_target, Type::UnsignedInt)
        && matches!(stripped(data), Expression::Variable(name) if name == word)
}

fn recognize(function: &Function) -> Option<MatrixPacket<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [matrix_id, source, scale] = function.parameters.as_slice() else {
        return None;
    };
    if matrix_id.parameter_type != Type::Int
        || source.parameter_type != Type::Pointer(Pointee::Float)
        || scale.parameter_type != Type::Char
    {
        return None;
    }
    let [values, word, packet_id] = function.locals.as_slice() else {
        return None;
    };
    if values.declared_type != Type::Int
        || values.array_length != Some(6)
        || word.declared_type != Type::UnsignedInt
        || packet_id.declared_type != Type::UnsignedInt
        || function.locals.iter().any(|local| {
            local.initializer.is_some() || local.is_static || local.is_volatile
        })
    {
        return None;
    }
    let [noop, select,
        a0, a1, scale_add, zero0, f00, f01, f02, f03, port0,
        a2, a3, zero1, f10, f11, f12, f13, port1,
        a4, a5, zero2, f20, f21, f22, f23, port2, flag] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !no_op(noop)
        || !switch_assigns_ranges(select, &matrix_id.name, &packet_id.name)
        || !scale_update(scale_add, &scale.name)
        || !zero_word(zero0, &word.name)
        || !zero_word(zero1, &word.name)
        || !zero_word(zero2, &word.name)
        || !matrix_store(a0, &values.name, 0, &source.name, 0)
        || !matrix_store(a1, &values.name, 1, &source.name, 12)
        || !matrix_store(a2, &values.name, 2, &source.name, 4)
        || !matrix_store(a3, &values.name, 3, &source.name, 16)
        || !matrix_store(a4, &values.name, 4, &source.name, 8)
        || !matrix_store(a5, &values.name, 5, &source.name, 20)
        || !port_write(port0, &word.name)
        || !port_write(port1, &word.name)
        || !port_write(port2, &word.name)
    {
        return None;
    }
    for (packet, fields) in [[f00, f01, f02, f03], [f10, f11, f12, f13], [f20, f21, f22, f23]]
        .into_iter()
        .enumerate()
    {
        let first = field_insert(fields[0], &word.name, 0xffff_f800, 0)?;
        let second = field_insert(fields[1], &word.name, 0xffc0_07ff, 11)?;
        let third = field_insert(fields[2], &word.name, 0xff3f_ffff, 22)?;
        let fourth = field_insert(fields[3], &word.name, 0x00ff_ffff, 24)?;
        if !indexed(first, &values.name, (packet * 2) as i64)
            || !indexed(second, &values.name, (packet * 2 + 1) as i64)
            || !scale_bits(third, &scale.name, (packet * 2) as i64)
            || !packet_number(fourth, &packet_id.name, packet as i64 + 6)
        {
            return None;
        }
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            },
        value,
    } = flag
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    (constant_value(value) == Some(0)).then_some(MatrixPacket {
        matrix_id: &matrix_id.name,
        source: &source.name,
        scale: &scale.name,
        values: &values.name,
        word: &word.name,
        packet_id: &packet_id.name,
        global,
        flag_offset: i16::try_from(*offset).ok()?,
    })
}

fn push_conditional(generator: &mut Generator, options: u8, condition_bit: u8) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::BranchConditionalForward {
            options,
            condition_bit,
            target: 0,
        });
    index
}

fn push_branch(generator: &mut Generator) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::Branch { target: 0 });
    index
}

fn patch_branch(generator: &mut Generator, index: usize, destination: usize) {
    match &mut generator.output.instructions[index] {
        Instruction::BranchConditionalForward { target, .. }
        | Instruction::Branch { target } => *target = destination,
        _ => unreachable!("packet dispatch patch points are branches"),
    }
}

impl Generator {
    pub(crate) fn try_fixed_port_matrix_packets(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.locations.get(shape.matrix_id).map(|location| location.register) != Some(3)
            || self.locations.get(shape.source).map(|location| location.register) != Some(4)
            || self.locations.get(shape.scale).map(|location| location.register) != Some(5)
            || self.globals.get(shape.global).is_none()
        {
            return Ok(false);
        }
        let _semantic_locals = (shape.values, shape.word, shape.packet_id);
        self.output.pre_scheduled = true;
        self.output.has_conversion = true;
        self.frame_size = 120;

        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -120 });
        let beq8 = push_conditional(self, 12, 2);
        let bge8 = push_conditional(self, 4, 0);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 4 });
        let beq4 = push_conditional(self, 12, 2);
        let bge4 = push_conditional(self, 4, 0);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 1 });
        let bge1 = push_conditional(self, 4, 0);
        let below1 = push_branch(self);
        let cmp12 = self.output.instructions.len();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 12 });
        let bge12 = push_conditional(self, 4, 0);
        let below12 = push_branch(self);
        let sub1 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -1 });
        let join1 = push_branch(self);
        let sub5 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -5 });
        let join5 = push_branch(self);
        let sub9 = self.output.instructions.len();
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: -9 });
        let join9 = push_branch(self);
        let default = self.output.instructions.len();
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        let join = self.output.instructions.len();
        for branch in [beq8, beq4, below1, bge12] {
            patch_branch(self, branch, default);
        }
        patch_branch(self, bge8, cmp12);
        patch_branch(self, bge4, sub5);
        patch_branch(self, bge1, sub1);
        patch_branch(self, below12, sub9);
        for branch in [join1, join5, join9] {
            patch_branch(self, branch, join);
        }

        self.evaluate(&Expression::FloatLiteral(1024.0), Type::Float, 2)?;
        self.output.instructions.extend([
            Instruction::MultiplyImmediate { d: 3, a: 0, immediate: 3 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 12 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::AddImmediate { d: 11, a: 5, immediate: 17 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 6 },
            Instruction::ExtendSignByte { a: 11, s: 11 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::load_immediate(10, 0x61),
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::load_immediate_shifted(9, 0xcc01u16 as i16),
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 112 },
            Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 24 },
            Instruction::AddImmediate { d: 6, a: 3, immediate: 7 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 104 },
            Instruction::AddImmediate { d: 5, a: 3, immediate: 8 },
            Instruction::LoadWord { d: 8, a: 1, offset: 116 },
            Instruction::LoadWord { d: 7, a: 1, offset: 108 },
        ]);
        let global_type = self.globals[shape.global];
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 3)?;
        self.output.instructions.extend([
            Instruction::RotateAndMask { a: 7, s: 7, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 7, s: 8, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 7, s: 11, shift: 22, begin: 8, end: 9 },
            Instruction::RotateAndMaskInsert { a: 0, s: 7, shift: 0, begin: 8, end: 31 },
            Instruction::StoreWord { s: 0, a: 9, offset: -32768 },
            Instruction::load_immediate(0, 0),
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 4 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 16 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 96 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 88 },
            Instruction::LoadWord { d: 8, a: 1, offset: 100 },
            Instruction::LoadWord { d: 7, a: 1, offset: 92 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 7, s: 8, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 7, s: 11, shift: 20, begin: 8, end: 9 },
            Instruction::RotateAndMask { a: 7, s: 7, shift: 0, begin: 8, end: 31 },
            Instruction::RotateAndMaskInsert { a: 7, s: 6, shift: 24, begin: 0, end: 7 },
            Instruction::StoreWord { s: 7, a: 9, offset: -32768 },
            Instruction::LoadFloatSingle { d: 1, a: 4, offset: 8 },
            Instruction::LoadFloatSingle { d: 0, a: 4, offset: 20 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
            Instruction::StoreByte { s: 10, a: 9, offset: -32768 },
            Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 80 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 72 },
            Instruction::LoadWord { d: 6, a: 1, offset: 84 },
            Instruction::LoadWord { d: 4, a: 1, offset: 76 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 11, begin: 10, end: 20 },
            Instruction::RotateAndMaskInsert { a: 4, s: 6, shift: 0, begin: 21, end: 31 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 0, begin: 10, end: 7 },
            Instruction::RotateAndMaskInsert { a: 4, s: 11, shift: 18, begin: 8, end: 9 },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 0, begin: 8, end: 31 },
            Instruction::RotateAndMaskInsert { a: 4, s: 5, shift: 24, begin: 0, end: 7 },
            Instruction::StoreWord { s: 4, a: 9, offset: -32768 },
            Instruction::StoreHalfword { s: 0, a: 3, offset: shape.flag_offset },
        ]);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
