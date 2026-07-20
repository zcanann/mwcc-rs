//! Two-bit enum remapping followed by a state-member bitfield and dirty-bit update.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower the SDK enum permutation `((x >> 1) & 1) | (x << 1)`, its insertion into one state
    /// word, and a trailing dirty-bit OR. Build 163 forms the permutation in r6 with `slwi` plus
    /// `rlwimi`, preserving the global state base in r4 across both member updates.
    pub(crate) fn try_enum_remap_member_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::Int {
            return Ok(false);
        }
        let [data, remapped] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(data.declared_type, Type::StructPointer { .. })
            || data.initializer.is_some()
            || data.array_length.is_some()
            || data.is_static
            || remapped.declared_type != Type::Int
            || remapped.initializer.is_some()
            || remapped.array_length.is_some()
            || remapped.is_static
        {
            return Ok(false);
        }

        let [assign_data, assign_low_bit, assign_high_bit, field_store, dirty_store] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Statement::Assign {
            name: data_name,
            value: Expression::Variable(global),
        } = assign_data
        else {
            return Ok(false);
        };
        if data_name != &data.name {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };

        let Statement::Assign {
            name: low_name,
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left: low_shift,
                    right: low_mask,
                },
        } = assign_low_bit
        else {
            return Ok(false);
        };
        if low_name != &remapped.name
            || constant_value(low_mask) != Some(1)
            || !matches!(low_shift.as_ref(), Expression::Binary { operator: BinaryOperator::ShiftRight, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &parameter.name)
                    && constant_value(right) == Some(1))
        {
            return Ok(false);
        }
        let Statement::Assign {
            name: high_name,
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: preserved,
                    right: high_shift,
                },
        } = assign_high_bit
        else {
            return Ok(false);
        };
        if high_name != &remapped.name
            || !matches!(preserved.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &remapped.name)
                    && constant_value(right) == Some(-3))
            || !matches!(high_shift.as_ref(), Expression::Binary { operator: BinaryOperator::ShiftLeft, left, right }
                if matches!(left.as_ref(), Expression::Cast { operand, .. }
                    if matches!(operand.as_ref(), Expression::Variable(name) if name == &parameter.name))
                    && constant_value(right) == Some(1))
        {
            return Ok(false);
        }

        let Statement::Store {
            target:
                Expression::Member {
                    base: field_base,
                    offset: field_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: field_preserved,
                    right: field_insert,
                },
        } = field_store
        else {
            return Ok(false);
        };
        if !matches!(field_base.as_ref(), Expression::Variable(name) if name == &data.name) {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: old_field,
            right: field_mask,
        } = field_preserved.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: inserted,
            right: field_shift,
        } = field_insert.as_ref()
        else {
            return Ok(false);
        };
        let Some(preserve_mask) = constant_value(field_mask) else {
            return Ok(false);
        };
        let Some((preserve_begin, preserve_end)) = rlwinm_mask(preserve_mask) else {
            return Ok(false);
        };
        let Some(field_shift) = constant_value(field_shift).and_then(|value| u8::try_from(value).ok())
        else {
            return Ok(false);
        };
        if !matches!(old_field.as_ref(), Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
                if offset == field_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == &data.name))
            || !matches!(inserted.as_ref(), Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == &remapped.name))
        {
            return Ok(false);
        }

        let Statement::Store {
            target:
                Expression::Member {
                    base: dirty_base,
                    offset: dirty_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: old_dirty,
                    right: dirty_bit,
                },
        } = dirty_store
        else {
            return Ok(false);
        };
        let Some(dirty_bit) = constant_value(dirty_bit).and_then(|value| u16::try_from(value).ok())
        else {
            return Ok(false);
        };
        if !matches!(dirty_base.as_ref(), Expression::Variable(name) if name == &data.name)
            || !matches!(old_dirty.as_ref(), Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
                if offset == dirty_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == &data.name))
        {
            return Ok(false);
        }
        let (Ok(field_displacement), Ok(dirty_displacement)) =
            (i16::try_from(*field_offset), i16::try_from(*dirty_offset))
        else {
            return Ok(false);
        };

        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(global.clone()), global_type, 4)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 6, s: 3, shift: 1 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 6,
                s: 3,
                shift: 31,
                begin: 31,
                end: 31,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: field_displacement,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin: preserve_begin,
            end: preserve_end,
        });
        self.output.instructions.push(Instruction::ShiftLeftImmediate {
            a: 0,
            s: 6,
            shift: field_shift,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: field_displacement,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: dirty_displacement,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: dirty_bit,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: dirty_displacement,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
