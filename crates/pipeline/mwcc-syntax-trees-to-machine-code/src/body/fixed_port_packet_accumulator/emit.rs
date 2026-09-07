//! Emission for a recognized fixed-port packet accumulator.

use super::recognize::{recognize, FieldOperation, Packet};
#[allow(unused_imports)]
use super::super::*;

impl Generator {
    /// Lower the full-width ten-field GX BP packet schedule.  Recognition is
    /// structural: the fields must cover the measured bit ranges and consume
    /// the matching ABI parameters, independent of function or local names.
    pub(crate) fn try_fixed_port_packet_accumulator(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(packet) = recognize(function) else {
            return Ok(false);
        };
        let expected_shifts = [0, 2, 4, 7, 9, 13, 16, 19, 20, 24];
        let expected_widths = [2, 2, 3, 2, 4, 3, 3, 1, 1, 8];
        if packet.fields.len() != expected_shifts.len()
            || packet.fields.iter().any(|field| field.operation != packet.fields[0].operation)
            || packet
                .fields
                .iter()
                .zip(expected_shifts)
                .zip(expected_widths)
                .any(|((field, shift), width)| {
                    field.shift != shift
                        || (!field.preserve_mask)
                            != (((1u32 << width) - 1).wrapping_shl(u32::from(shift)))
                })
        {
            return Ok(false);
        }
        let expected_parameters = [1usize, 2, 3, 9, 4, 5, 6, 8, 7, 0];
        if function.parameters.len() != 10
            || packet
                .fields
                .iter()
                .zip(expected_parameters)
                .enumerate()
                .any(|(field_index, (field, parameter_index))| {
                    field.source.parameter != function.parameters[parameter_index].name
                        || field.source.addend != if field_index == 9 { 16 } else { 0 }
                })
            || function.parameters[..8]
                .iter()
                .enumerate()
                .any(|(index, parameter)| {
                    self.locations
                        .get(&parameter.name)
                        .map(|location| location.register)
                        != Some(Eabi::FIRST_GENERAL_ARGUMENT + index as u8)
                })
            || function.parameters.iter().enumerate().any(|(index, parameter)| {
                if matches!(index, 7 | 8) {
                    parameter.parameter_type != Type::UnsignedChar
                } else {
                    !matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)
                }
            })
        {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(packet.global) else {
            return Ok(false);
        };
        if !matches!(global_type, Type::Pointer(_) | Type::StructPointer { .. })
            || self.volatile_globals.contains(packet.global)
        {
            return Ok(false);
        }
        if packet.fields[0].operation == FieldOperation::RotateInsert {
            if !self.fixed_address_objects.values().any(|&address| address == packet.port) {
                return Ok(false);
            }
            self.emit_intrinsic_packet(&packet, global_type)?;
            return Ok(true);
        }
        let preserve = |index: usize| {
            rlwinm_mask(packet.fields[index].preserve_mask as i64)
                .expect("recognized field preserve mask is encodable")
        };
        let (field_1_begin, field_1_end) = preserve(1);
        let (field_2_begin, field_2_end) = preserve(2);
        let (field_3_begin, field_3_end) = preserve(3);
        let (field_4_begin, field_4_end) = preserve(4);
        let (field_5_begin, field_5_end) = preserve(5);
        let (field_6_begin, field_6_end) = preserve(6);
        let (field_7_begin, field_7_end) = preserve(7);
        let (field_8_begin, field_8_end) = preserve(8);
        let (low_begin, low_end) =
            contiguous_mask(packet.fields[9].preserve_mask as i64)
                .expect("recognized final field leaves one contiguous low packet");
        let port_high = (packet.port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = packet.port as u16 as i16;

        self.output.pre_scheduled = true;
        self.frame_size = 40;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: field_1_begin,
            end: field_1_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 5, shift: 2 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadWord { d: 11, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 0,
            shift: 0,
            begin: field_2_begin,
            end: field_2_end,
        });
        self.output
            .instructions
            .push(Instruction::LoadByteZero { d: 12, a: 1, offset: 51 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 6, shift: 4 });
        self.evaluate(
            &Expression::Variable(packet.global.to_string()),
            global_type,
            4,
        )?;
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 0,
            shift: 0,
            begin: field_3_begin,
            end: field_3_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 11, shift: 7 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 0,
            shift: 0,
            begin: field_4_begin,
            end: field_4_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 7, shift: 9 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 0,
            shift: 0,
            begin: field_5_begin,
            end: field_5_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 8, shift: 13 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: i16::try_from(packet.fields[9].source.addend)
                .expect("recognized addend fits"),
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin: field_6_begin,
            end: field_6_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 9, shift: 16 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin: field_7_begin,
            end: field_7_end,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 12, shift: 19 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin: field_8_begin,
            end: field_8_end,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 10,
            shift: 20,
            begin: 4,
            end: 11,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 6, s: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: packet.command,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: port_high,
            });
        self.output
            .instructions
            .push(Instruction::StoreByte { s: 0, a: 3, offset: port_low });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 5, shift: 24 });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 0,
                s: 6,
                shift: 0,
                begin: low_begin,
                end: low_end,
            });
        self.output
            .instructions
            .push(Instruction::StoreWord { s: 0, a: 3, offset: port_low });
        self.output
            .instructions
            .push(Instruction::AddImmediate { d: 0, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 4,
            offset: packet.flag_offset,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_intrinsic_packet(&mut self, packet: &Packet<'_>, global_type: Type) -> Compilation<()> {
        self.output.pre_scheduled = true;
        self.frame_size = 48;
        self.callee_saved = vec![31];
        let incoming_byte = self.frame_size
            + Eabi::general_stack_offset(Eabi::LAST_GENERAL_ARGUMENT + 1).expect("ninth word") + 3;
        let incoming_word = self.frame_size
            + Eabi::general_stack_offset(Eabi::LAST_GENERAL_ARGUMENT + 2).expect("tenth word");
        let insert = |index: usize, destination, source| {
            let field = packet.fields[index];
            let (begin, end) = contiguous_mask((!field.preserve_mask) as i64)
                .expect("recognized insert mask is contiguous");
            Instruction::RotateAndMaskInsert {
                a: destination, s: source, shift: field.shift, begin, end,
            }
        };
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -self.frame_size },
            Instruction::load_immediate(0, 0),
            insert(0, 0, 4),
            Instruction::StoreWord { s: 31, a: 1, offset: self.frame_size - 4 },
            Instruction::move_register(11, 0),
            insert(1, 11, 5),
            Instruction::LoadWord { d: 12, a: 1, offset: incoming_word },
            Instruction::LoadByteZero { d: 31, a: 1, offset: incoming_byte },
            insert(2, 11, 6),
        ]);
        self.evaluate(&Expression::Variable(packet.global.into()), global_type, 4)?;
        self.output.instructions.extend([
            insert(3, 11, 12),
            insert(4, 11, 7),
            insert(5, 11, 8),
            insert(6, 11, 9),
            insert(7, 11, 31),
            Instruction::load_immediate(0, packet.command),
            Instruction::load_immediate_shifted(5, (packet.port.wrapping_add(0x8000) >> 16) as i16),
            Instruction::StoreByte { s: 0, a: 5, offset: packet.port as i16 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: packet.fields[9].source.addend as i16 },
            insert(8, 11, 10),
            insert(9, 11, 0),
            Instruction::StoreWord { s: 11, a: 5, offset: packet.port as i16 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword { s: 0, a: 4, offset: packet.flag_offset },
        ]);
        self.emit_epilogue_and_return();
        Ok(())
    }

}
