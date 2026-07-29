//! Count-register loops for large aggregate member assignments.

#[allow(unused_imports)]
use super::*;

fn aggregate_member_copy_shape<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<(&'a Expression, u32, &'a Expression, u32, u32)> {
    let Expression::Member {
        base: destination,
        offset: destination_offset,
        member_type: Type::Struct {
            size: destination_size,
            ..
        },
        index_stride: None,
    } = target
    else {
        return None;
    };
    let Expression::Member {
        base: source,
        offset: source_offset,
        member_type: Type::Struct {
            size: source_size, ..
        },
        index_stride: None,
    } = value
    else {
        return None;
    };
    (*destination_size == *source_size
        && *source_size >= 16
        && *source_size % 8 == 0
        && *destination_offset >= 8
        && *source_offset >= 8)
        .then_some((
            destination,
            *destination_offset,
            source,
            *source_offset,
            *source_size,
        ))
}

impl Generator {
    /// Copy a one- or two-word aggregate member whose concrete offset proves
    /// word alignment. Compact parser layouts retain aggregate type identity
    /// even when MWCC scalarizes the assignment to `lwz`/`stw` pairs.
    pub(crate) fn try_emit_small_aggregate_member_copy(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let (
            Expression::Member {
                base: destination,
                offset: destination_offset,
                member_type:
                    Type::Struct {
                        size: destination_size,
                        ..
                    },
                index_stride: None,
            },
            Expression::Member {
                base: source,
                offset: source_offset,
                member_type:
                    Type::Struct {
                        size: source_size, ..
                    },
                index_stride: None,
            },
        ) = (target, value)
        else {
            return Ok(false);
        };
        if destination_size != source_size
            || !matches!(*source_size, 4 | 8)
            || destination_offset % 4 != 0
            || source_offset % 4 != 0
        {
            return Ok(false);
        }
        let destination_base = self.general_register_of_leaf(destination)?;
        let source_base = self.general_register_of_leaf(source)?;
        let first_word = self.fresh_virtual_general_preferring(if *source_size == 8 {
            Eabi::FIRST_GENERAL_ARGUMENT
        } else {
            GENERAL_SCRATCH
        });
        let destination_offset = i16::try_from(*destination_offset)
            .map_err(|_| Diagnostic::error("aggregate destination member is out of range"))?;
        let source_offset = i16::try_from(*source_offset)
            .map_err(|_| Diagnostic::error("aggregate source member is out of range"))?;
        self.output.instructions.push(Instruction::LoadWord {
            d: first_word,
            a: source_base,
            offset: source_offset,
        });
        let second_offsets = if *source_size == 8 {
            Some((
                source_offset
                    .checked_add(4)
                    .ok_or_else(|| Diagnostic::error("aggregate source member is out of range"))?,
                destination_offset.checked_add(4).ok_or_else(|| {
                    Diagnostic::error("aggregate destination member is out of range")
                })?,
            ))
        } else {
            None
        };
        let second_word = second_offsets.map(|(second_source_offset, _)| {
            let register = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: source_base,
                offset: second_source_offset,
            });
            register
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: first_word,
            a: destination_base,
            offset: destination_offset,
        });
        if let (Some(second_word), Some((_, second_destination_offset))) =
            (second_word, second_offsets)
        {
            self.output.instructions.push(Instruction::StoreWord {
                s: second_word,
                a: destination_base,
                offset: second_destination_offset,
            });
        }
        Ok(true)
    }

    /// Copy a large, eight-byte-aligned aggregate member two words per CTR
    /// iteration. MWCC points both update-form bases one doubleword before the
    /// member, then pairs `lwzu/lwz` with `stwu/stw`.
    pub(crate) fn try_emit_aggregate_member_copy_loop(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((destination, destination_offset, source, source_offset, byte_count)) =
            aggregate_member_copy_shape(target, value)
        else {
            return Ok(false);
        };
        let destination_base = self.general_register_of_leaf(destination)?;
        let source_base = self.general_register_of_leaf(source)?;
        let destination_address = self.fresh_virtual_general_preferring(5);
        let source_address = self.fresh_virtual_general_preferring(4);
        let first_word = self.fresh_virtual_general_preferring(3);
        let iterations = i16::try_from(byte_count / 8)
            .map_err(|_| Diagnostic::error("aggregate member copy is too large"))?;
        let destination_displacement = i16::try_from(destination_offset - 8)
            .map_err(|_| Diagnostic::error("aggregate destination member is out of range"))?;
        let source_displacement = i16::try_from(source_offset - 8)
            .map_err(|_| Diagnostic::error("aggregate source member is out of range"))?;

        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, iterations));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: GENERAL_SCRATCH });
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: destination_address,
                a: destination_base,
                immediate: destination_displacement,
            },
            Instruction::AddImmediate {
                d: source_address,
                a: source_base,
                immediate: source_displacement,
            },
        ]);
        let loop_head = self.fresh_label();
        self.bind_label(loop_head);
        self.output.instructions.extend([
            Instruction::LoadWordWithUpdate {
                d: first_word,
                a: source_address,
                offset: 8,
            },
            Instruction::LoadWord {
                d: GENERAL_SCRATCH,
                a: source_address,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: first_word,
                a: destination_address,
                offset: 8,
            },
            Instruction::StoreWord {
                s: GENERAL_SCRATCH,
                a: destination_address,
                offset: 4,
            },
        ]);
        self.emit_branch_conditional_to(16, 0, loop_head);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, offset: u32, size: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(name.into())),
            offset,
            member_type: Type::Struct { size, align: 4 },
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_a_two_word_stride_member_copy() {
        let destination = member("dst", 1568, 80);
        let source = member("src", 1568, 80);
        let shape = aggregate_member_copy_shape(&destination, &source);
        assert!(matches!(shape, Some((_, 1568, _, 1568, 80))));
        assert!(
            aggregate_member_copy_shape(&member("dst", 4, 12), &member("src", 4, 12)).is_none()
        );
    }

    #[test]
    fn keeps_small_aggregate_members_out_of_the_loop_shape() {
        let destination = member("dst", 6356, 4);
        let source = member("src", 6356, 4);
        assert!(aggregate_member_copy_shape(&destination, &source).is_none());
        let destination = member("dst", 9008, 8);
        let source = member("src", 9008, 8);
        assert!(aggregate_member_copy_shape(&destination, &source).is_none());
    }
}
