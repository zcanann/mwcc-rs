//! Measured multi-instruction schedules for direct-call arguments.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Without O4 latency scheduling, simple global/constant arguments remain
    /// in source order. This is deliberately separate from the O4 rules below:
    /// no instruction may run ahead of an earlier argument in this path.
    pub(crate) fn try_emit_unscheduled_global_constant_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        if !direct_call
            || self.behavior.schedule_latency_slots
            || arguments.is_empty()
            || arguments.len() > 8
            || !arguments.iter().all(|argument| match argument {
                Expression::IntegerLiteral(_) => true,
                Expression::Variable(name) => {
                    self.globals.contains_key(name.as_str())
                        || self.global_array_sizes.contains_key(name.as_str())
                }
                _ => false,
            })
        {
            return Ok(false);
        }

        for (position, argument) in arguments.iter().enumerate() {
            self.evaluate_general(argument, Eabi::FIRST_GENERAL_ARGUMENT + position as u8)?;
        }
        Ok(true)
    }

    /// Schedule `(short_global, i16[, wide_i32])` under absolute addressing.
    ///
    /// Both address/constant high halves run first. Their dependent low halves
    /// then alternate, and the halfword load waits until immediately before the
    /// call. The final LR-save pass moves the two leading materializations into
    /// the non-leaf prologue's latency slots.
    pub(crate) fn try_emit_absolute_short_global_constant_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let (global, middle, wide) = match arguments {
            [
                Expression::Variable(global),
                Expression::IntegerLiteral(middle),
            ] => (global, middle, None),
            [
                Expression::Variable(global),
                Expression::IntegerLiteral(middle),
                Expression::IntegerLiteral(wide),
            ] => (global, middle, Some(wide)),
            _ => return Ok(false),
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || self.globals.get(global.as_str()) != Some(&Type::Short)
            || !(i16::MIN as i64..=i16::MAX as i64).contains(middle)
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        let second = first + 1;
        let wide_parts = wide.map(|wide| {
            let wide = *wide as i32;
            let low = (wide as u32 & 0xffff) as i16;
            let high_adjusted = ((wide - low as i32) >> 16) as i16;
            (wide, high_adjusted, low)
        });
        if let Some((wide, high_adjusted, low)) = wide_parts {
            if (-0x8000..=0x7fff).contains(&wide) || low == 0 {
                return Ok(false);
            }
        }

        self.emit_address_high(first, global);
        if let Some((_, high_adjusted, _)) = wide_parts {
            self.output.instructions.push(Instruction::load_immediate_shifted(
                first + 2,
                high_adjusted,
            ));
        }

        self.emit_address_low(first, global);
        self.output
            .instructions
            .push(Instruction::load_immediate(second, *middle as i16));
        if let Some((_, _, low)) = wide_parts {
            self.output.instructions.push(Instruction::AddImmediate {
                d: first + 2,
                a: first + 2,
                immediate: low,
            });
        }
        self.output.instructions.push(self.global_load_instruction(
            Type::Short,
            first,
            first,
        )?);
        Ok(true)
    }
}
