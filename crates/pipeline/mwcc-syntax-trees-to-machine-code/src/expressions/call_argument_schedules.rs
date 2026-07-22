//! Measured multi-instruction schedules for direct-call arguments.

#[allow(unused_imports)]
use super::*;

fn direct_member_address(expression: &Expression) -> Option<(&Expression, u32)> {
    match expression {
        Expression::MemberAddress {
            base,
            offset,
            index_stride: None,
            ..
        } => Some((base.as_ref(), *offset)),
        Expression::AddressOf { operand } => match operand.as_ref() {
            Expression::Member {
                base,
                offset,
                index_stride: None,
                ..
            } => Some((base.as_ref(), *offset)),
            _ => None,
        },
        _ => None,
    }
}

impl Generator {
    /// Preserve an incoming first parameter when constructing a global-member
    /// receiver for argument zero would otherwise overwrite its `r3` home
    /// before argument one takes the address of one of its members.
    ///
    /// MWCC uses the first register beyond the two argument slots as the
    /// temporary (`mr r5,r3; ...global address in r3...; addi r4,r5,offset`).
    /// This is both an observed schedule and a correctness requirement: using
    /// r3 for the final addi would address the global object instead.
    pub(crate) fn try_emit_global_member_and_endangered_member_address(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let [first, second] = arguments else {
            return Ok(false);
        };
        if !direct_call {
            return Ok(false);
        }

        let Some((first_base, _)) = direct_member_address(first) else {
            return Ok(false);
        };
        let Some((second_base, second_offset)) = direct_member_address(second) else {
            return Ok(false);
        };
        let Expression::Variable(global) = first_base else {
            return Ok(false);
        };
        let Expression::Variable(parameter) = second_base else {
            return Ok(false);
        };
        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        if !self.globals.contains_key(global.as_str())
            || self
                .locations
                .get(parameter.as_str())
                .map(|location| location.register)
                != Some(first_argument)
        {
            return Ok(false);
        }
        let second_offset = i16::try_from(second_offset).map_err(|_| {
            Diagnostic::error("member address argument offset out of range (roadmap)")
        })?;
        let preserved = first_argument + 2;
        self.emit_integer_materialization_copy(preserved, first_argument);
        self.evaluate_general(first, first_argument)?;
        if second_offset == 0 {
            self.output.instructions.push(Instruction::move_register(
                first_argument + 1,
                preserved,
            ));
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: first_argument + 1,
                a: preserved,
                immediate: second_offset,
            });
        }
        Ok(true)
    }

    /// Under latency scheduling, an i16 constant in the second argument slot is
    /// independent of a first argument loaded from a structure member. MWCC
    /// issues the `li r4` first, allowing the linkage scheduler to consume it,
    /// then performs the potentially dependent `lwz r3` immediately before the
    /// call. This order is stable from build 163 through the later mainline.
    pub(crate) fn try_emit_member_constant_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            first @ Expression::Member {
                base,
                member_type,
                ..
            },
            Expression::IntegerLiteral(value),
        ] = arguments
        else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !matches!(base.as_ref(), Expression::Variable(_))
            || matches!(
                member_type,
                Type::Float
                    | Type::Double
                    | Type::LongLong
                    | Type::UnsignedLongLong
                    | Type::Void
                    | Type::Struct { .. }
            )
            || !(i16::MIN as i64..=i16::MAX as i64).contains(value)
        {
            return Ok(false);
        }

        self.evaluate_general(
            &Expression::IntegerLiteral(*value),
            Eabi::FIRST_GENERAL_ARGUMENT + 1,
        )?;
        self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        Ok(true)
    }

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
