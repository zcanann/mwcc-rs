//! Shared-zero stores, a bit-field update, and a guarded initialization tail.
//!
//! This is one scheduler region: MWCC retains the leading zero in the first
//! free argument register across the bit-field merge and reuses it after the
//! record-form field test.

#[allow(unused_imports)]
use super::*;
use crate::expressions::{displacement_load, displacement_store};

struct MemberStore<'a> {
    base: &'a str,
    offset: i16,
    member_type: Type,
}

struct BitField<'a> {
    storage: MemberStore<'a>,
    shift: u8,
    width: u8,
}

struct LeadingSharedZeroBitfieldGuard<'a> {
    base: &'a str,
    bit_value: &'a str,
    float_value: &'a str,
    float_store: MemberStore<'a>,
    leading_zero_stores: [MemberStore<'a>; 2],
    bit_field: BitField<'a>,
    guarded_zero_store: MemberStore<'a>,
    guarded_global_store: MemberStore<'a>,
    global: &'a str,
    global_member_offset: i16,
    global_member_type: Type,
}

fn member_store(expression: &Expression) -> Option<MemberStore<'_>> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some(MemberStore {
        base,
        offset: i16::try_from(*offset).ok()?,
        member_type: *member_type,
    })
}

fn bit_field(expression: &Expression) -> Option<BitField<'_>> {
    let Expression::BitFieldRead {
        storage,
        shift,
        width,
        ..
    } = expression
    else {
        return None;
    };
    let storage = member_store(storage)?;
    if *width == 0
        || u16::from(*shift) + u16::from(*width) > u16::from(storage.member_type.width())
    {
        return None;
    }
    Some(BitField {
        storage,
        shift: *shift,
        width: *width,
    })
}

fn same_member(left: &MemberStore<'_>, right: &MemberStore<'_>) -> bool {
    left.base == right.base
        && left.offset == right.offset
        && left.member_type == right.member_type
}

fn zero_member_store(statement: &Statement) -> Option<MemberStore<'_>> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    (constant_value(value) == Some(0))
        .then(|| member_store(target))
        .flatten()
}

fn classify(function: &Function) -> Option<LeadingSharedZeroBitfieldGuard<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, bit_value, float_value] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::StructPointer { .. })
        || bit_value.parameter_type != Type::Int
        || float_value.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::Store {
        target: float_target,
        value: Expression::Variable(stored_float),
    }, first_zero, second_zero, Statement::Store {
        target: stored_field,
        value: Expression::Variable(stored_bit),
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let float_store = member_store(float_target)?;
    if float_store.base != base.name
        || float_store.member_type != Type::Float
        || stored_float != &float_value.name
        || stored_bit != &bit_value.name
        || !else_body.is_empty()
    {
        return None;
    }
    let first_zero = zero_member_store(first_zero)?;
    let second_zero = zero_member_store(second_zero)?;
    if first_zero.base != base.name || second_zero.base != base.name {
        return None;
    }
    let stored_field = bit_field(stored_field)?;
    let tested_field = bit_field(condition)?;
    if stored_field.storage.base != base.name
        || !same_member(&stored_field.storage, &tested_field.storage)
        || stored_field.shift != tested_field.shift
        || stored_field.width != tested_field.width
    {
        return None;
    }
    let [guarded_zero, Statement::Store {
        target: global_target,
        value: global_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let guarded_zero_store = zero_member_store(guarded_zero)?;
    let guarded_global_store = member_store(global_target)?;
    if guarded_zero_store.base != base.name || guarded_global_store.base != base.name {
        return None;
    }
    let Expression::Member {
        base: global_base,
        offset: global_member_offset,
        member_type: global_member_type,
        index_stride: None,
    } = global_value
    else {
        return None;
    };
    let Expression::Variable(global) = global_base.as_ref() else {
        return None;
    };
    Some(LeadingSharedZeroBitfieldGuard {
        base: &base.name,
        bit_value: &bit_value.name,
        float_value: &float_value.name,
        float_store,
        leading_zero_stores: [first_zero, second_zero],
        bit_field: stored_field,
        guarded_zero_store,
        guarded_global_store,
        global,
        global_member_offset: i16::try_from(*global_member_offset).ok()?,
        global_member_type: *global_member_type,
    })
}

impl Generator {
    pub(crate) fn try_leading_shared_zero_bitfield_guard(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(false);
        }
        let base = self.general_register_of(shape.base)?;
        let bit_value = self.general_register_of(shape.bit_value)?;
        let float_value = self.float_register_of(shape.float_value)?;
        if base != 3 || bit_value != 4 || float_value != 1 {
            return Ok(false);
        }
        let zero = 5;
        let storage_pointee = pointee_of_type(shape.bit_field.storage.member_type)
            .ok_or_else(|| Diagnostic::error("bit-field guard has no scalar storage width"))?;
        let global_pointee = pointee_of_type(shape.global_member_type)
            .ok_or_else(|| Diagnostic::error("guarded global member has no scalar load width"))?;
        self.output.pre_scheduled = true;

        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: float_value,
            a: base,
            offset: shape.float_store.offset,
        });
        self.load_integer_constant(zero, 0);
        for store in &shape.leading_zero_stores {
            let pointee = pointee_of_type(store.member_type)
                .ok_or_else(|| Diagnostic::error("leading zero has no scalar store width"))?;
            self.output.instructions.push(displacement_store(
                pointee,
                zero,
                base,
                store.offset,
            )?);
        }
        self.output.instructions.push(displacement_load(
            storage_pointee,
            GENERAL_SCRATCH,
            base,
            shape.bit_field.storage.offset,
        )?);
        let begin = 32 - shape.bit_field.shift - shape.bit_field.width;
        let end = 31 - shape.bit_field.shift;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: GENERAL_SCRATCH,
                s: bit_value,
                shift: shape.bit_field.shift,
                begin,
                end,
            });
        self.output.instructions.push(displacement_store(
            storage_pointee,
            GENERAL_SCRATCH,
            base,
            shape.bit_field.storage.offset,
        )?);
        self.output.instructions.push(displacement_load(
            storage_pointee,
            GENERAL_SCRATCH,
            base,
            shape.bit_field.storage.offset,
        )?);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                shift: 32 - shape.bit_field.shift,
                begin: 32 - shape.bit_field.width,
                end: 31,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        let guarded_zero_pointee = pointee_of_type(shape.guarded_zero_store.member_type)
            .ok_or_else(|| Diagnostic::error("guarded zero has no scalar store width"))?;
        self.output.instructions.push(displacement_store(
            guarded_zero_pointee,
            zero,
            base,
            shape.guarded_zero_store.offset,
        )?);
        self.record_relocation(RelocationKind::EmbSda21, shape.global);
        self.output.instructions.push(Instruction::LoadWord {
            d: bit_value,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(displacement_load(
            global_pointee,
            GENERAL_SCRATCH,
            bit_value,
            shape.global_member_offset,
        )?);
        let guarded_global_pointee = pointee_of_type(shape.guarded_global_store.member_type)
            .ok_or_else(|| Diagnostic::error("guarded global store has no scalar width"))?;
        self.output.instructions.push(displacement_store(
            guarded_global_pointee,
            GENERAL_SCRATCH,
            base,
            shape.guarded_global_store.offset,
        )?);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
