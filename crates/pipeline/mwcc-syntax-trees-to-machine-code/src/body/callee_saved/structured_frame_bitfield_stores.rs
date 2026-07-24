//! Register and issue scheduling for frame-array bitfield stores.
//!
//! MWCC computes a whole size-optimized branch arm into a small volatile
//! register fanout before committing its bytes on newer allocators; the legacy
//! allocator alternates computation and stores to reuse r0, with r3 carrying
//! the one overlapping value. This module owns that arm-local schedule.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_frame_store_before_if_branch(&mut self, branch: usize) {
        if branch < 2 {
            return;
        }
        if matches!(self.output.instructions[branch - 2], Instruction::StoreByte { a: 1, .. })
            && matches!(
                self.output.instructions[branch - 1],
                Instruction::CompareWordImmediate { .. }
                    | Instruction::CompareLogicalWordImmediate { .. }
            )
        {
            self.output.instructions.swap(branch - 2, branch - 1);
        }
    }

    pub(super) fn try_emit_structured_frame_bitfield_stores(
        &mut self,
        statements: &[Statement],
    ) -> Compilation<bool> {
        if !matches!(statements.len(), 2 | 4) {
            return Ok(false);
        }
        let mut stores = Vec::with_capacity(statements.len());
        for statement in statements {
            let Statement::Store {
                target: Expression::Index { base, index },
                value,
            } = statement
            else {
                return Ok(false);
            };
            let Expression::Variable(array) = base.as_ref() else {
                return Ok(false);
            };
            let Some(slot) = self
                .frame_slots
                .get(array)
                .copied()
                .filter(|slot| slot.is_array)
            else {
                return Ok(false);
            };
            let Some(index) = constant_value(index) else {
                return Ok(false);
            };
            let offset = i16::try_from(i64::from(slot.offset) + index)
                .map_err(|_| Diagnostic::error("frame-array byte offset is out of range"))?;
            stores.push((value, offset));
        }

        if self.behavior.power_pc_7400_scheduling_enabled() {
            match stores.as_slice() {
                [first, second, third, fourth] => {
                    self.evaluate(first.0, Type::UnsignedInt, 0)?;
                    self.evaluate(second.0, Type::UnsignedInt, 4)?;
                    self.emit_frame_array_byte_store(0, first.1);
                    self.evaluate(third.0, Type::UnsignedInt, 3)?;
                    self.evaluate(fourth.0, Type::UnsignedInt, 0)?;
                    self.emit_frame_array_byte_store(4, second.1);
                    self.emit_frame_array_byte_store(3, third.1);
                    self.emit_frame_array_byte_store(0, fourth.1);
                }
                [first, second] => {
                    self.evaluate(first.0, Type::UnsignedInt, 3)?;
                    self.evaluate(second.0, Type::UnsignedInt, 0)?;
                    self.emit_frame_array_byte_store(3, first.1);
                    self.emit_frame_array_byte_store(0, second.1);
                }
                _ => unreachable!("store count was validated above"),
            }
            return Ok(true);
        }

        match (self.behavior.frame_convention, stores.as_slice()) {
            (FrameConvention::Predecrement, stores) => {
                let registers: &[u8] = if stores.len() == 4 {
                    &[5, 4, 3, 0]
                } else {
                    &[3, 0]
                };
                for ((value, _), &register) in stores.iter().zip(registers) {
                    self.evaluate(value, Type::UnsignedInt, register)?;
                }
                for ((_, offset), &register) in stores.iter().zip(registers) {
                    self.emit_frame_array_byte_store(register, *offset);
                }
            }
            (FrameConvention::LinkageFirst, [first, second, third, fourth]) => {
                self.evaluate(first.0, Type::UnsignedInt, 0)?;
                self.emit_frame_array_byte_store(0, first.1);
                self.evaluate(second.0, Type::UnsignedInt, 0)?;
                self.evaluate(third.0, Type::UnsignedInt, 3)?;
                self.emit_frame_array_byte_store(0, second.1);
                self.evaluate(fourth.0, Type::UnsignedInt, 0)?;
                self.emit_frame_array_byte_store(3, third.1);
                self.emit_frame_array_byte_store(0, fourth.1);
            }
            (FrameConvention::LinkageFirst, [first, second]) => {
                self.evaluate(first.0, Type::UnsignedInt, 0)?;
                self.emit_frame_array_byte_store(0, first.1);
                self.evaluate(second.0, Type::UnsignedInt, 0)?;
                self.emit_frame_array_byte_store(0, second.1);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn emit_frame_array_byte_store(&mut self, source: u8, offset: i16) {
        self.output.instructions.push(Instruction::StoreByte {
            s: source,
            a: 1,
            offset,
        });
        self.written_slots.insert(offset);
    }
}
