//! Shared recognition and emission primitives for 64-bit body families.

use super::*;

#[derive(Clone, Copy)]
pub(crate) enum ClockRead<'a> {
    Absolute(u32),
    Global(&'a str),
}

pub(crate) fn unsigned_word_clock(expression: &Expression) -> Option<ClockRead<'_>> {
    let expression = match expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } => operand.as_ref(),
        other => other,
    };
    match expression {
        Expression::Variable(name) => Some(ClockRead::Global(name)),
        Expression::Dereference { pointer } => match pointer.as_ref() {
            Expression::Cast {
                target_type: Type::Pointer(Pointee::UnsignedInt),
                operand,
            } => constant_value(operand)
                .and_then(|address| u32::try_from(address).ok())
                .map(ClockRead::Absolute),
            _ => None,
        },
        _ => None,
    }
}

impl Generator {
    pub(crate) fn supports_unsigned_word_clock(&self, clock: ClockRead<'_>) -> bool {
        match clock {
            ClockRead::Absolute(_) => true,
            ClockRead::Global(name) => {
                matches!(self.globals.get(name), Some(Type::UnsignedInt))
                    && self.behavior.global_addressing == GlobalAddressing::SmallData
            }
        }
    }

    pub(crate) fn emit_unsigned_word_clock_high(&mut self, clock: ClockRead<'_>, register: u8) {
        if let ClockRead::Absolute(address) = clock {
            let (high, _) = crate::expressions::split_address(address);
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(register, high));
        }
    }

    pub(crate) fn emit_unsigned_word_clock_load(&mut self, clock: ClockRead<'_>, register: u8) {
        match clock {
            ClockRead::Absolute(address) => {
                let (_, low) = crate::expressions::split_address(address);
                self.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: register,
                    offset: low,
                });
            }
            ClockRead::Global(name) => {
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: 0,
                    offset: 0,
                });
            }
        }
    }
}
