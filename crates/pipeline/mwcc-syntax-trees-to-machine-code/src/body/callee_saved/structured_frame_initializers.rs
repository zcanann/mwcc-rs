//! Copy-in for initialized automatic arrays in structured frames.
//!
//! Parser images contain only explicitly initialized bytes; C's aggregate
//! rules zero-fill the remainder of the declared array. This owner performs
//! that expansion after incoming values have reached their saved homes and
//! before the first body statement can observe the frame slots.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn emit_structured_frame_array_initializers(
        &mut self,
        arrays: &[&LocalDeclaration],
    ) -> Compilation<()> {
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
