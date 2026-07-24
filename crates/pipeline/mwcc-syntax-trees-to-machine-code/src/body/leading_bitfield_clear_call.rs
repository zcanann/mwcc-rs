//! A zero bit-field update through a pointer alias before a call on its owner.
//!
//! The alias is only an address-generation value: the original object remains
//! live in its incoming argument register for the trailing call.  Claim this
//! shape before immutable-alias inlining, which would otherwise erase that
//! distinction and make the generic expression emitter reuse the call register
//! as the member base.

#[allow(unused_imports)]
use super::*;
use crate::expressions::{displacement_load, displacement_store};

struct LeadingBitfieldClearCall<'a> {
    object: &'a str,
    alias_offset: i16,
    storage_offset: i16,
    storage_type: Type,
    shift: u8,
    width: u8,
    callee: &'a str,
}

fn classify(function: &Function) -> Option<LeadingBitfieldClearCall<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let [alias] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(alias.declared_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let Some(Expression::Member {
        base: alias_base,
        offset: alias_offset,
        index_stride: None,
        ..
    }) = alias.initializer.as_ref()
    else {
        return None;
    };
    if !matches!(alias_base.as_ref(), Expression::Variable(name) if name == &object.name) {
        return None;
    }
    let [
        Statement::Store {
            target:
                Expression::BitFieldRead {
                    storage,
                    shift,
                    width,
                    ..
                },
            value,
        },
        Statement::Expression(Expression::Call { name: callee, arguments }),
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if constant_value(value) != Some(0)
        || !matches!(arguments.as_slice(), [Expression::Variable(name)] if name == &object.name)
    {
        return None;
    }
    let Expression::Member {
        base: storage_base,
        offset: storage_offset,
        member_type: storage_type,
        index_stride: None,
    } = storage.as_ref()
    else {
        return None;
    };
    if !matches!(storage_base.as_ref(), Expression::Variable(name) if name == &alias.name)
        || !matches!(
            storage_type,
            Type::UnsignedChar | Type::UnsignedShort | Type::UnsignedInt
        )
        || *width == 0
        || u16::from(*shift) + u16::from(*width) > u16::from(storage_type.width())
    {
        return None;
    }
    Some(LeadingBitfieldClearCall {
        object: &object.name,
        alias_offset: i16::try_from(*alias_offset).ok()?,
        storage_offset: i16::try_from(*storage_offset).ok()?,
        storage_type: *storage_type,
        shift: *shift,
        width: *width,
        callee,
    })
}

impl Generator {
    pub(crate) fn try_leading_bitfield_clear_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let object = self.general_register_of(shape.object)?;
        if object != 3 || self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let pointee = pointee_of_type(shape.storage_type)
            .ok_or_else(|| Diagnostic::error("bit-field clear has no scalar storage width"))?;
        let alias = 4;
        let zero = 5;

        self.output.pre_scheduled = true;
        self.emit_plain_nonleaf_prologue();
        self.load_integer_constant(zero, 0);
        let materialize_zero = self
            .output
            .instructions
            .pop()
            .expect("zero materialization was just emitted");
        self.output.instructions.insert(1, materialize_zero);
        self.output.instructions.push(Instruction::LoadWord {
            d: alias,
            a: object,
            offset: shape.alias_offset,
        });
        self.output.instructions.push(displacement_load(
            pointee,
            GENERAL_SCRATCH,
            alias,
            shape.storage_offset,
        )?);
        let begin = 32 - shape.shift - shape.width;
        let end = 31 - shape.shift;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: GENERAL_SCRATCH,
                s: zero,
                shift: shape.shift,
                begin,
                end,
            });
        self.output.instructions.push(displacement_store(
            pointee,
            GENERAL_SCRATCH,
            alias,
            shape.storage_offset,
        )?);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
