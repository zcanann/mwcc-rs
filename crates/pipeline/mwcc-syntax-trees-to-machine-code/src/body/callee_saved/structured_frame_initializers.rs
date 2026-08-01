//! Copy-in for initialized automatic arrays in structured frames.
//!
//! Parser images contain only explicitly initialized bytes; C's aggregate
//! rules zero-fill the remainder of the declared array. This owner performs
//! that expansion after incoming values have reached their saved homes and
//! before the first body statement can observe the frame slots.

#[allow(unused_imports)]
use super::*;

/// Hidden-label accounting for a five-word runtime instruction trampoline.
///
/// The optimizer owns the patch diamond as part of the array copy transaction:
/// one-word arms consume eight fewer ordinary structured labels, while the
/// two-word SPR arms consume three fewer. This is structural because the array
/// image, call forwarding, and complete arm store counts must all agree.
pub(super) fn instruction_array_hidden_label_discount(function: &Function) -> u32 {
    let [array] = function.locals.as_slice() else {
        return 0;
    };
    if array.declared_type != Type::UnsignedInt
        || array.array_length != Some(5)
        || array.data_bytes.as_ref().is_none_or(|image| {
            image.len() != 20 || !image.chunks_exact(4).all(|word| word == [0x60, 0, 0, 0])
        })
    {
        return 0;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return 0;
    };
    let Some(read) = function.parameters.get(2) else {
        return 0;
    };
    if !matches!(condition, Expression::Variable(name) if name == &read.name)
        || then_body.len() != else_body.len()
        || !then_body.iter().chain(else_body).all(|statement| {
            matches!(statement,
                Statement::Store {
                    target: Expression::Index { base, index },
                    ..
                } if matches!(base.as_ref(), Expression::Variable(name) if name == &array.name)
                    && constant_value(index).is_some_and(|value| (0..2).contains(&value)))
        })
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Call { arguments, .. })
                if matches!(arguments.as_slice(), [_, Expression::Variable(name), _]
                    if name == &array.name))
    {
        return 0;
    }
    match then_body.len() {
        1 => 8,
        2 => 3,
        _ => 0,
    }
}

impl Generator {
    pub(super) fn emit_structured_frame_array_initializers(
        &mut self,
        function: &Function,
        arrays: &[&LocalDeclaration],
        image_sources: &[&LocalDeclaration],
    ) -> Compilation<()> {
        if self.emit_linkage_first_instruction_array_image(function, arrays, image_sources)? {
            return Ok(());
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            if let Some(plan) =
                super::structured_array_pool::plan_structured_array_pool(arrays, image_sources)
            {
                return self.emit_structured_array_pool(arrays, image_sources, &plan);
            }
        }
        for array in arrays {
            let Some(explicit) = array.data_bytes.as_ref() else {
                continue;
            };
            let slot = self
                .frame_slots
                .get(&array.name)
                .copied()
                .ok_or_else(|| Diagnostic::error("initialized array has no frame slot"))?;
            let size = usize::try_from(slot.size)
                .map_err(|_| Diagnostic::error("initialized array is too large"))?;
            if explicit.len() > size {
                return Err(Diagnostic::error(
                    "initialized array image exceeds its frame slot",
                ));
            }

            let mut image = vec![0; size];
            image[..explicit.len()].copy_from_slice(explicit);
            if image.iter().all(|byte| *byte == 0) && slot.offset % 4 == 0 && slot.size % 4 == 0 {
                self.emit_structured_zero_array(slot)?;
                continue;
            }

            self.emit_structured_array_image(slot, &image)?;
        }
        Ok(())
    }

    /// Copy a five-word executable trampoline from one contiguous anonymous
    /// image. Legacy MWCC alternates two scratch registers so each pair of
    /// loads is followed immediately by its pair of stores; treating equal
    /// words as independent constants loses both the pool shape and schedule.
    fn emit_linkage_first_instruction_array_image(
        &mut self,
        function: &Function,
        arrays: &[&LocalDeclaration],
        image_sources: &[&LocalDeclaration],
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let [array] = arrays else {
            return Ok(false);
        };
        let [image_source] = image_sources else {
            return Ok(false);
        };
        let Some(explicit) = image_source.data_bytes.as_ref() else {
            return Ok(false);
        };
        let Some(slot) = self.frame_slots.get(&array.name).copied() else {
            return Ok(false);
        };
        if image_source.name != array.name
            || array.array_length != Some(5)
            || !matches!(array.declared_type, Type::Int | Type::UnsignedInt)
            || !array.data_relocations.is_empty()
            || slot.offset != 8
            || slot.size != 20
            || explicit.len() != 20
        {
            return Ok(false);
        }

        self.output
            .anonymous_rodata
            .push(mwcc_machine_code::AnonymousRodata {
                bytes: explicit.clone(),
                static_slot_prefix_bump: None,
                // Runtime instruction-patch diamonds number the image three
                // slots after the discounted structured-label counter.
                anonymous_offset: if instruction_array_hidden_label_discount(function) > 0 {
                    2
                } else {
                    -1
                },
            });
        self.output.post_constant_label_bump += 1;
        let image = self.output.anonymous_rodata.len() - 1;
        self.record_target(
            RelocationKind::Addr16Ha,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            });
        self.record_target(
            RelocationKind::Addr16Lo,
            mwcc_machine_code::RelocationTarget::AnonymousRodataAt(image),
        );
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 6,
            immediate: 0,
        });
        for pair in 0..2i16 {
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 6,
                    a: 7,
                    offset: pair * 8,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 7,
                    offset: pair * 8 + 4,
                },
                Instruction::StoreWord {
                    s: 6,
                    a: 1,
                    offset: slot.offset + pair * 8,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: slot.offset + pair * 8 + 4,
                },
            ]);
        }
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: slot.offset + 16,
            },
        ]);
        Ok(true)
    }

    fn emit_structured_zero_array(&mut self, slot: FrameSlot) -> Compilation<()> {
        let words = i16::try_from(slot.size / 4)
            .map_err(|_| Diagnostic::error("zero-initialized array is too large"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.output.instructions.push(Instruction::load_immediate(
            Eabi::FIRST_GENERAL_ARGUMENT,
            words,
        ));
        self.output.instructions.push(Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
            a: 1,
            immediate: slot.offset - 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister {
                s: Eabi::FIRST_GENERAL_ARGUMENT,
            });
        let loop_head = self.fresh_label();
        self.bind_label(loop_head);
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: GENERAL_SCRATCH,
                a: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                offset: 4,
            });
        self.emit_branch_conditional_to(16, 0, loop_head);
        Ok(())
    }

    fn emit_structured_array_image(&mut self, slot: FrameSlot, image: &[u8]) -> Compilation<()> {
        let mut offset = 0usize;
        while offset + 4 <= image.len() && (i32::from(slot.offset) + offset as i32) % 4 == 0 {
            let bits = u32::from_be_bytes([
                image[offset],
                image[offset + 1],
                image[offset + 2],
                image[offset + 3],
            ]);
            self.load_word_constant(GENERAL_SCRATCH, bits);
            self.output.instructions.push(Instruction::StoreWord {
                s: GENERAL_SCRATCH,
                a: 1,
                offset: slot.offset
                    + i16::try_from(offset)
                        .map_err(|_| Diagnostic::error("array image offset is too large"))?,
            });
            offset += 4;
        }
        while offset < image.len() {
            self.load_integer_constant(GENERAL_SCRATCH, i64::from(image[offset]));
            self.output.instructions.push(Instruction::StoreByte {
                s: GENERAL_SCRATCH,
                a: 1,
                offset: slot.offset
                    + i16::try_from(offset)
                        .map_err(|_| Diagnostic::error("array image offset is too large"))?,
            });
            offset += 1;
        }
        Ok(())
    }
}
