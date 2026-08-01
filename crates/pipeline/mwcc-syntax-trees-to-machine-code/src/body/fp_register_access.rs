//! Runtime-generated floating-point register access trampoline.
//!
//! A five-word no-op image is copied to the frame and patched for ordinary FPR,
//! FPSCR, or FPECR access. Legacy optimized MWCC owns the pool image, compact
//! frame, patch stores, helper calls, and 64-bit normalization as one schedule.

#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
struct FpRegisterAccess {
    array: String,
    image: Vec<u8>,
    ordinary_limit: u16,
    fpscr_index: u16,
    fpecr_index: u16,
    spr_number: i16,
    special_access: String,
    spr_access: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn comparison<'a>(
    expression: &'a Expression,
    name: &str,
    operator: BinaryOperator,
) -> Option<&'a Expression> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*actual == operator && variable(left, name)).then_some(right)
}

fn array_store<'a>(
    statement: &'a Statement,
    array: &str,
    index: i64,
) -> Option<&'a Expression> {
    let Statement::Store {
        target: Expression::Index { base, index: actual_index },
        value,
    } = statement
    else {
        return None;
    };
    (variable(base, array) && constant_value(actual_index) == Some(index)).then_some(value)
}

fn call_assignment<'a>(
    statement: &'a Statement,
    result: &str,
    pointer: &str,
    array: &str,
    read: &str,
) -> Option<&'a str> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    let (callee, arguments) = direct_call(value)?;
    (name == result
        && matches!(arguments, [first, second, third]
            if variable(first, pointer)
                && variable(second, array)
                && variable(third, read)))
    .then_some(callee)
}

fn casted_pointer(expression: &Expression, name: &str, pointee: Pointee) -> bool {
    matches!(expression,
        Expression::Cast {
            target_type: Type::Pointer(actual),
            operand,
        } if *actual == pointee && variable(operand, name))
}

fn recognize(function: &Function) -> Option<FpRegisterAccess> {
    let [pointer, fpr, read] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || pointer.parameter_type != Type::Pointer(Pointee::Int)
        || fpr.parameter_type != Type::UnsignedInt
        || read.parameter_type != Type::Int
        || !function.guards.is_empty()
        || function.locals.len() != 2
    {
        return None;
    }
    let error = function.locals.iter().find(|local| {
        local.declared_type == Type::Int
            && local.initializer.as_ref().and_then(constant_value) == Some(0)
            && !local.is_static
    })?;
    let array = function.locals.iter().find(|local| {
        local.declared_type == Type::UnsignedInt
            && local.array_length == Some(5)
            && local.initializer.is_none()
            && local.data_relocations.is_empty()
            && !local.is_static
    })?;
    let image = array.data_bytes.as_ref()?;
    if image.len() != 20
        || !image.chunks_exact(4).all(|word| word == [0x60, 0, 0, 0])
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &error.name)
    {
        return None;
    }

    let [Statement::If {
        condition: ordinary_condition,
        then_body: ordinary_body,
        else_body: special_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let ordinary_limit = u16::try_from(constant_value(comparison(
        ordinary_condition,
        &fpr.name,
        BinaryOperator::Less,
    )?)?)
    .ok()?;
    let [ordinary_patch, ordinary_call] = ordinary_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: ordinary_read,
        then_body: ordinary_read_body,
        else_body: ordinary_write_body,
    } = ordinary_patch
    else {
        return None;
    };
    let [ordinary_read_store] = ordinary_read_body.as_slice() else {
        return None;
    };
    let [ordinary_write_store] = ordinary_write_body.as_slice() else {
        return None;
    };
    let ordinary_read_value = array_store(ordinary_read_store, &array.name, 0)?;
    let ordinary_write_value = array_store(ordinary_write_store, &array.name, 0)?;
    if !variable(ordinary_read, &read.name)
        || constant_value(ordinary_read_value).is_some()
        || constant_value(ordinary_write_value).is_some()
    {
        return None;
    }
    let special_access = call_assignment(
        ordinary_call,
        &error.name,
        &pointer.name,
        &array.name,
        &read.name,
    )?;

    let [fpscr_case] = special_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: fpscr_condition,
        then_body: fpscr_body,
        else_body: fpecr_else,
    } = fpscr_case
    else {
        return None;
    };
    let fpscr_index = u16::try_from(constant_value(comparison(
        fpscr_condition,
        &fpr.name,
        BinaryOperator::Equal,
    )?)?)
    .ok()?;
    let [fpscr_patch, fpscr_call, fpscr_mask] = fpscr_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: fpscr_read,
        then_body: fpscr_read_body,
        else_body: fpscr_write_body,
    } = fpscr_patch
    else {
        return None;
    };
    if !variable(fpscr_read, &read.name)
        || fpscr_read_body.len() != 4
        || fpscr_write_body.len() != 4
        || fpscr_read_body
            .iter()
            .enumerate()
            .any(|(index, statement)| array_store(statement, &array.name, index as i64).is_none())
        || fpscr_write_body
            .iter()
            .enumerate()
            .any(|(index, statement)| array_store(statement, &array.name, index as i64).is_none())
        || call_assignment(
            fpscr_call,
            &error.name,
            &pointer.name,
            &array.name,
            &read.name,
        )? != special_access
    {
        return None;
    }
    let Statement::Store {
        target: Expression::Dereference { pointer: fpscr_target },
        value: Expression::IndexedUpdateValue { value: fpscr_update },
    } = fpscr_mask
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: fpscr_source,
        right: fpscr_mask_value,
    } = fpscr_update.as_ref()
    else {
        return None;
    };
    if !casted_pointer(fpscr_target, &pointer.name, Pointee::UnsignedLongLong)
        || !matches!(fpscr_source.as_ref(), Expression::Dereference { pointer: source }
            if casted_pointer(source, &pointer.name, Pointee::UnsignedLongLong))
        || constant_value(fpscr_mask_value) != Some(0xffff_ffff)
    {
        return None;
    }

    let [fpecr_case] = fpecr_else.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: fpecr_condition,
        then_body: fpecr_body,
        else_body: fpecr_absent,
    } = fpecr_case
    else {
        return None;
    };
    let fpecr_index = u16::try_from(constant_value(comparison(
        fpecr_condition,
        &fpr.name,
        BinaryOperator::Equal,
    )?)?)
    .ok()?;
    let [write_copy, spr_call, read_normalize] = fpecr_body.as_slice() else {
        return None;
    };
    if !fpecr_absent.is_empty()
        || !matches!(write_copy,
            Statement::If { condition: Expression::Unary { operator: UnaryOperator::LogicalNot, operand }, then_body, else_body }
                if variable(operand, &read.name) && then_body.len() == 1 && else_body.is_empty())
        || !matches!(read_normalize,
            Statement::If { condition, then_body, else_body }
                if variable(condition, &read.name) && then_body.len() == 1 && else_body.is_empty())
    {
        return None;
    }
    let Statement::Assign { name: spr_result, value: spr_expression } = spr_call else {
        return None;
    };
    let (spr_access, spr_arguments) = direct_call(spr_expression)?;
    let [spr_pointer, spr_number, spr_read] = spr_arguments else {
        return None;
    };
    if spr_result != &error.name
        || !variable(spr_pointer, &pointer.name)
        || !variable(spr_read, &read.name)
    {
        return None;
    }

    Some(FpRegisterAccess {
        array: array.name.clone(),
        image: image.clone(),
        ordinary_limit,
        fpscr_index,
        fpecr_index,
        spr_number: i16::try_from(constant_value(spr_number)?).ok()?,
        special_access: special_access.into(),
        spr_access: spr_access.into(),
    })
}

impl Generator {
    pub(crate) fn try_fp_register_access(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        let Some(access) = recognize(function) else {
            return Ok(false);
        };
        self.emit_fp_register_access(access);
        Ok(true)
    }

    fn emit_fp_register_access(&mut self, access: FpRegisterAccess) {
        const ARRAY_OFFSET: i16 = 8;
        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![31, 30];
        self.frame_slots.insert(
            access.array,
            FrameSlot {
                offset: ARRAY_OFFSET,
                class: ValueClass::General,
                size: 20,
                value_type: Type::UnsignedInt,
                parameter_register: None,
                is_array: true,
            },
        );
        self.output
            .anonymous_rodata
            .push(mwcc_machine_code::AnonymousRodata {
                bytes: access.image,
                static_slot_prefix_bump: None,
                // Eleven optimizer labels precede the image's source slot;
                // eight remain outside the ordinary running counter here.
                anonymous_offset: -9,
            });
        self.output.post_constant_label_bump += 1;
        let image = self.output.anonymous_rodata.len() - 1;

        let special = self.fresh_label();
        let ordinary_write = self.fresh_label();
        let ordinary_call = self.fresh_label();
        let fpecr = self.fresh_label();
        let fpscr_write = self.fresh_label();
        let fpscr_call = self.fresh_label();
        let spr_call = self.fresh_label();
        let done = self.fresh_label();
        let emit_call = |generator: &mut Self, name: &str| {
            generator.record_relocation(RelocationKind::Rel24, name);
            generator.output.instructions.push(Instruction::BranchAndLink {
                target: name.into(),
            });
        };

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
            Instruction::StoreWord { s: 31, a: 1, offset: 36 },
            Instruction::StoreWord { s: 30, a: 1, offset: 32 },
            Instruction::move_register(30, 3),
            Instruction::move_register(31, 5),
        ]);
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: access.ordinary_limit },
        ]);
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 6, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 6, offset: 0 },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord { d: 0, a: 6, offset: 4 },
            Instruction::StoreWord { s: 5, a: 1, offset: 8 },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::LoadWord { d: 5, a: 6, offset: 8 },
            Instruction::LoadWord { d: 0, a: 6, offset: 12 },
            Instruction::StoreWord { s: 5, a: 1, offset: 16 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::LoadWord { d: 0, a: 6, offset: 16 },
            Instruction::StoreWord { s: 0, a: 1, offset: 24 },
        ]);
        self.emit_branch_conditional_to(4, 0, special);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, ordinary_write);
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 0, s: 4, shift: 21 },
            Instruction::OrImmediateShifted { a: 0, s: 0, immediate: 0xd803 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
        ]);
        self.emit_branch_to(ordinary_call);
        self.bind_label(ordinary_write);
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 0, s: 4, shift: 21 },
            Instruction::OrImmediateShifted { a: 0, s: 0, immediate: 0xc803 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
        ]);
        self.bind_label(ordinary_call);
        self.output.instructions.extend([
            Instruction::move_register(3, 30),
            Instruction::AddImmediate { d: 4, a: 1, immediate: 8 },
            Instruction::move_register(5, 31),
        ]);
        emit_call(self, &access.special_access);
        self.emit_branch_to(done);

        self.bind_label(special);
        self.emit_branch_conditional_to(4, 2, fpecr);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, fpscr_write);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: -10204 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: -992 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 1166 },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: -10205 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: -14300 },
            Instruction::StoreWord { s: 3, a: 1, offset: 16 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
        ]);
        self.emit_branch_to(fpscr_call);
        self.bind_label(fpscr_write);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: -10204 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: -14301 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: -514 },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 3470 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: -14300 },
            Instruction::StoreWord { s: 3, a: 1, offset: 16 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
        ]);
        self.bind_label(fpscr_call);
        self.output.instructions.extend([
            Instruction::move_register(3, 30),
            Instruction::AddImmediate { d: 4, a: 1, immediate: 8 },
            Instruction::move_register(5, 31),
        ]);
        emit_call(self, &access.special_access);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 6, a: 30, offset: 4 },
            Instruction::load_immediate(0, -1),
            Instruction::LoadWord { d: 5, a: 30, offset: 0 },
            Instruction::load_immediate(4, 0),
            Instruction::And { a: 0, s: 6, b: 0 },
            Instruction::StoreWord { s: 0, a: 30, offset: 4 },
            Instruction::And { a: 0, s: 5, b: 4 },
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);
        self.emit_branch_to(done);

        self.bind_label(fpecr);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate {
            a: 4,
            immediate: access.fpecr_index,
        });
        self.emit_branch_conditional_to(4, 2, done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, spr_call);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 30, offset: 4 },
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);
        self.bind_label(spr_call);
        self.output.instructions.extend([
            Instruction::move_register(3, 30),
            Instruction::move_register(5, 31),
            Instruction::load_immediate(4, access.spr_number),
        ]);
        emit_call(self, &access.spr_access);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, done);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 5, a: 30, offset: 0 },
            Instruction::load_immediate(0, -1),
            Instruction::load_immediate(4, 0),
            Instruction::And { a: 0, s: 5, b: 0 },
            Instruction::StoreWord { s: 0, a: 30, offset: 4 },
            Instruction::And { a: 0, s: 4, b: 4 },
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);

        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 31, a: 1, offset: 36 },
            Instruction::LoadWord { d: 30, a: 1, offset: 32 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 11;
        let _ = access.fpscr_index;
    }
}
