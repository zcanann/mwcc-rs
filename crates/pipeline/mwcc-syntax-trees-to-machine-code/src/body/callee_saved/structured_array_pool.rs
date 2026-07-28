//! Mainline pooled copy-in for a run of zero-initialized automatic arrays.
//!
//! Planning is independent of frame offsets so the structured frame owner can
//! reserve the fixed saved-register suffix before assigning slots. Emission
//! owns the anonymous images, late section displacements, direct copy window,
//! and optional eight-byte CTR tail loop as one transaction.

#[allow(unused_imports)]
use super::*;

const POOLED_COPY_REGISTERS: [u8; 24] = [
    0, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 12, 11, 10, 9, 8, 7, 6, 5,
];

pub(super) struct StructuredArrayPoolPlan {
    pub(super) direct_word_count: usize,
    pub(super) loop_array_index: Option<usize>,
    pub(super) first_saved_register: u8,
}

pub(super) fn plan_structured_array_pool(
    arrays: &[&LocalDeclaration],
) -> Option<StructuredArrayPoolPlan> {
    if arrays.len() < 2
        || arrays.iter().any(|array| {
            array
                .data_bytes
                .as_ref()
                .is_none_or(|bytes| bytes.is_empty() || bytes.iter().any(|byte| *byte != 0))
        })
    {
        return None;
    }

    let byte_sizes: Vec<usize> = arrays
        .iter()
        .map(|array| usize::try_from(super::structured_frame_arrays::array_byte_size(array)?).ok())
        .collect::<Option<_>>()?;
    if byte_sizes.iter().any(|size| *size == 0 || size % 4 != 0) {
        return None;
    }

    let total_words = byte_sizes.iter().sum::<usize>() / 4;
    let (direct_word_count, loop_array_index) = if total_words <= POOLED_COPY_REGISTERS.len() {
        (total_words, None)
    } else {
        let loop_array_index = arrays.len() - 1;
        let direct_bytes = byte_sizes[..loop_array_index].iter().sum::<usize>();
        let loop_bytes = byte_sizes[loop_array_index];
        if direct_bytes / 4 > POOLED_COPY_REGISTERS.len() || loop_bytes % 8 != 0 {
            return None;
        }
        (direct_bytes / 4, Some(loop_array_index))
    };

    let first_saved_register = if loop_array_index.is_some() {
        14
    } else {
        POOLED_COPY_REGISTERS[..direct_word_count]
            .iter()
            .copied()
            .filter(|register| (14..=31).contains(register))
            .min()
            .unwrap_or(31)
    };
    Some(StructuredArrayPoolPlan {
        direct_word_count,
        loop_array_index,
        first_saved_register,
    })
}

impl Generator {
    pub(super) fn emit_structured_array_pool_base_high(&mut self) {
        self.record_relocation(RelocationKind::Addr16Ha, "...rodata.0");
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            });
    }

    pub(super) fn emit_structured_array_pool_base_low(&mut self) {
        self.record_relocation(RelocationKind::Addr16Lo, "...rodata.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 0,
        });
    }

    pub(super) fn emit_structured_array_pool(
        &mut self,
        arrays: &[&LocalDeclaration],
        plan: &StructuredArrayPoolPlan,
    ) -> Compilation<()> {
        let images = self.materialize_structured_array_pool_images(arrays)?;
        let first_blob = self.output.anonymous_rodata.len();
        for image in &images {
            self.output
                .anonymous_rodata
                .push(mwcc_machine_code::AnonymousRodata {
                    bytes: image.clone(),
                    anonymous_offset: 0,
                });
        }

        self.emit_structured_array_pool_loads(arrays, &images, plan, first_blob)?;
        self.emit_structured_array_pool_stores(arrays, &images, plan)?;
        if plan.loop_array_index.is_some() {
            self.emit_structured_array_pool_tail_loop();
        }
        Ok(())
    }

    fn materialize_structured_array_pool_images(
        &self,
        arrays: &[&LocalDeclaration],
    ) -> Compilation<Vec<Vec<u8>>> {
        let mut images = Vec::with_capacity(arrays.len());
        for array in arrays {
            let explicit = array
                .data_bytes
                .as_ref()
                .expect("pooled arrays were classified as initialized");
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
            images.push(image);
        }
        Ok(images)
    }

    fn emit_structured_array_pool_loads(
        &mut self,
        arrays: &[&LocalDeclaration],
        images: &[Vec<u8>],
        plan: &StructuredArrayPoolPlan,
        first_blob: usize,
    ) -> Compilation<()> {
        let direct_bytes = plan.direct_word_count * 4;
        if let Some(loop_array_index) = plan.loop_array_index {
            let source_offset = i16::try_from(direct_bytes.saturating_sub(4))
                .map_err(|_| Diagnostic::error("pooled array source offset is too large"))?;
            self.record_anonymous_rodata_displacement(first_blob);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 5,
                immediate: source_offset,
            });
            let iterations = i16::try_from(images[loop_array_index].len() / 8)
                .map_err(|_| Diagnostic::error("pooled array copy is too large"))?;
            self.output
                .instructions
                .push(Instruction::load_immediate(14, iterations));
            let slot = self.frame_slots[&arrays[loop_array_index].name];
            self.output.instructions.push(Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: slot.offset - 4,
            });
        }

        for (word_index, register) in POOLED_COPY_REGISTERS
            .iter()
            .copied()
            .take(plan.direct_word_count)
            .enumerate()
        {
            self.record_anonymous_rodata_displacement(first_blob);
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: 5,
                offset: i16::try_from(word_index * 4)
                    .map_err(|_| Diagnostic::error("pooled array load offset is too large"))?,
            });
        }
        Ok(())
    }

    fn emit_structured_array_pool_stores(
        &mut self,
        arrays: &[&LocalDeclaration],
        images: &[Vec<u8>],
        plan: &StructuredArrayPoolPlan,
    ) -> Compilation<()> {
        let mut word_index = 0usize;
        for (array_index, array) in arrays.iter().enumerate() {
            if Some(array_index) == plan.loop_array_index {
                break;
            }
            let slot = self.frame_slots[&array.name];
            for byte_offset in (0..images[array_index].len()).step_by(4) {
                let register = POOLED_COPY_REGISTERS[word_index];
                self.output.instructions.push(Instruction::StoreWord {
                    s: register,
                    a: 1,
                    offset: slot.offset
                        + i16::try_from(byte_offset).map_err(|_| {
                            Diagnostic::error("pooled array store offset is too large")
                        })?,
                });
                word_index += 1;
            }
        }
        debug_assert_eq!(word_index, plan.direct_word_count);
        Ok(())
    }

    fn emit_structured_array_pool_tail_loop(&mut self) {
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 14 });
        let loop_head = self.fresh_label();
        self.bind_label(loop_head);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 4,
            },
            Instruction::LoadWordWithUpdate {
                d: 0,
                a: 3,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 5,
                a: 4,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 0,
                a: 4,
                offset: 8,
            },
        ]);
        self.emit_branch_conditional_to(16, 0, loop_head);
    }

    fn record_anonymous_rodata_displacement(&mut self, blob: usize) {
        self.output
            .data_section_displacements
            .push(mwcc_machine_code::DataSectionDisplacement {
                instruction_index: self.output.instructions.len(),
                target: mwcc_machine_code::DataSectionDisplacementTarget::AnonymousRodata(blob),
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initialized_bytes(name: &str, length: u16) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::UnsignedChar,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(length),
            is_static: false,
            data_bytes: Some(vec![0]),
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn plans_fixed_register_words_then_a_large_tail_loop() {
        let date = initialized_bytes("date", 32);
        let time = initialized_bytes("time", 32);
        let ampm = initialized_bytes("ampm", 32);
        let buffer = initialized_bytes("buffer", 256);

        let plan = plan_structured_array_pool(&[&date, &time, &ampm, &buffer])
            .expect("the array run fits the mainline pooled-copy shape");

        assert_eq!(plan.direct_word_count, 24);
        assert_eq!(plan.loop_array_index, Some(3));
        assert_eq!(plan.first_saved_register, 14);
    }

    #[test]
    fn keeps_a_lone_initialized_array_on_inline_initialization() {
        let buffer = initialized_bytes("buffer", 32);
        assert!(plan_structured_array_pool(&[&buffer]).is_none());
    }

    #[test]
    fn rejects_a_prefix_larger_than_the_fixed_copy_window() {
        let first = initialized_bytes("first", 100);
        let second = initialized_bytes("second", 256);
        assert!(plan_structured_array_pool(&[&first, &second]).is_none());
    }

    #[test]
    fn leaves_nonzero_images_on_the_existing_initializer_path() {
        let mut first = initialized_bytes("first", 32);
        first.data_bytes = Some(vec![1]);
        let second = initialized_bytes("second", 32);
        assert!(plan_structured_array_pool(&[&first, &second]).is_none());
    }
}
