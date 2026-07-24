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

fn constant_indexed_address_base(expression: &Expression) -> Option<&Expression> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Index { base, index } = operand.as_ref() else {
        return None;
    };
    constant_value(index)?;
    Some(base.as_ref())
}

impl Generator {
    /// Marshal a word member followed by two constant-indexed addresses from
    /// the same pointer base.
    ///
    /// MWCC forms the address arguments right-to-left.  When their base is not
    /// endangered by the first member load, the member retains source order;
    /// otherwise both addresses must be formed before r3 is overwritten.  The
    /// distinction is observable both in small forwarding wrappers and after
    /// callee-saved pointer setup.
    pub(crate) fn try_emit_reverse_indexed_address_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            first @ Expression::Member {
                base: first_base,
                member_type,
                ..
            },
            second,
            third,
        ] = arguments
        else {
            return Ok(false);
        };
        let (Some(second_base), Some(third_base)) =
            (constant_indexed_address_base(second), constant_indexed_address_base(third))
        else {
            return Ok(false);
        };
        let word_member = matches!(
            member_type,
            Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. }
        );
        let all_general = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() >= 3
                && types[..3]
                    .iter()
                    .all(|ty| !matches!(ty, Type::Float | Type::Double))
        });
        let Some(first_base_register) = self.registers_used_by(first_base).into_iter().next() else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !word_member
            || !all_general
            || !structurally_equal(second_base, third_base)
        {
            return Ok(false);
        }

        if matches!(first_base_register, 0 | 3..=12) {
            self.evaluate_general(third, Eabi::FIRST_GENERAL_ARGUMENT + 2)?;
            self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
            self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        } else {
            self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
            self.evaluate_general(third, Eabi::FIRST_GENERAL_ARGUMENT + 2)?;
            self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        }
        Ok(true)
    }

    /// Fill a floating multiply's load latency with an independent word-member
    /// argument.  Both floating operands are placed first, the GPR load issues
    /// while their data becomes available, and the multiply completes directly
    /// before the call.
    pub(crate) fn try_emit_member_and_located_float_product_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            general @ Expression::Member { member_type, .. },
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left,
                right,
            },
        ] = arguments
        else {
            return Ok(false);
        };
        let expected_types = self.call_parameter_types.get(name).is_none_or(|types| {
            types.len() >= 2
                && !matches!(types[0], Type::Float | Type::Double)
                && matches!(types[1], Type::Float | Type::Double)
        });
        let word_member = matches!(
            member_type,
            Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. }
        );
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !expected_types
            || !word_member
            || !self.is_float_located(left)
            || !self.is_float_located(right)
        {
            return Ok(false);
        }

        let double = self.is_double_value(left) || self.is_double_value(right);
        let operands = self.place_float_operands(
            BinaryOperator::Multiply,
            left,
            right,
            Eabi::FIRST_FLOAT_ARGUMENT,
            double,
        )?;
        self.evaluate_general(general, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.output.instructions.push(float_combine(
            BinaryOperator::Multiply,
            Eabi::FIRST_FLOAT_ARGUMENT,
            operands,
            double,
        )?);
        Ok(true)
    }

    /// Marshal `(base->byte, base->bits[, saved])` while `base` still occupies
    /// the first argument register.
    ///
    /// The second byte load starts first, the saved leaf fills its latency
    /// slot, and only then may the first load overwrite r3.  The independent
    /// rotate completes immediately before the call.
    pub(crate) fn try_emit_shared_base_bitfield_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let (first, bit_field, third) = match arguments {
            [first, bit_field] => (first, bit_field, None),
            [first, bit_field, third @ Expression::Variable(_)] => {
                (first, bit_field, Some(third))
            }
            _ => return Ok(false),
        };
        let Expression::Member {
            base: first_base,
            member_type: Type::UnsignedChar,
            index_stride: None,
            ..
        } = first
        else {
            return Ok(false);
        };
        let Expression::BitFieldRead {
            storage,
            shift,
            width,
            ..
        } = bit_field
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: second_base,
            offset: second_offset,
            member_type: Type::UnsignedChar,
            index_stride: None,
        } = storage.as_ref()
        else {
            return Ok(false);
        };
        let (Expression::Variable(first_name), Expression::Variable(second_name)) =
            (first_base.as_ref(), second_base.as_ref())
        else {
            return Ok(false);
        };
        let third_info = match third {
            Some(third) => match self.leaf_info(third) {
                Ok(info) => Some(info),
                Err(_) => return Ok(false),
            },
            None => None,
        };
        let Some(shared_base) = self.lookup_general(first_name) else {
            return Ok(false);
        };
        if !direct_call
            || first_name != second_name
            || *width == 0
            || u16::from(*shift) + u16::from(*width) > 8
            // r4 is overwritten by the second load before the first member is
            // evaluated. r3 is safe because it is overwritten last; any other
            // shared base (including a callee-saved loop home) is independent.
            || shared_base == Eabi::FIRST_GENERAL_ARGUMENT + 1
            || third_info.is_some_and(|(register, width, _)| {
                width != 32 || register == Eabi::FIRST_GENERAL_ARGUMENT + 1
            })
        {
            return Ok(false);
        }

        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        let second_argument = first_argument + 1;
        let third_argument = first_argument + 2;
        self.emit_member_load(
            second_base,
            *second_offset,
            Type::UnsignedChar,
            None,
            second_argument,
        )?;
        if let Some(third) = third {
            self.evaluate_general(third, third_argument)?;
        }
        self.evaluate_general(first, first_argument)?;
        self.output.instructions.push(Instruction::RotateAndMask {
            a: second_argument,
            s: second_argument,
            shift: (32 - *shift) % 32,
            begin: 32 - *width,
            end: 31,
        });
        Ok(true)
    }

    /// Schedule `(large_string, i16, large_string)` without serializing the two
    /// address dependency chains. MWCC emits both high halves, completes the
    /// third argument through r4 into r5, then reuses r4 for the integer line
    /// number after completing r3.
    pub(crate) fn try_emit_large_string_line_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            Expression::StringLiteral(first),
            Expression::IntegerLiteral(line),
            Expression::StringLiteral(third),
        ] = arguments
        else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst
            || first.len() + 1 <= 8
            || third.len() + 1 <= 8
            || !(i16::MIN as i64..=i16::MAX as i64).contains(line)
        {
            return Ok(false);
        }

        let first = self.string_literal_placeholder(first);
        let third = self.string_literal_placeholder(third);
        self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT, &first);
        self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT + 1, &third);
        self.emit_string_address_low(
            &third,
            Eabi::FIRST_GENERAL_ARGUMENT + 1,
            Eabi::FIRST_GENERAL_ARGUMENT + 2,
        );
        self.emit_string_address_low(
            &first,
            Eabi::FIRST_GENERAL_ARGUMENT,
            Eabi::FIRST_GENERAL_ARGUMENT,
        );
        self.output.instructions.push(Instruction::load_immediate(
            Eabi::FIRST_GENERAL_ARGUMENT + 1,
            *line as i16,
        ));
        Ok(true)
    }

    /// Marshal `(member_y, ABS(member_x))` with the conditional argument first.
    ///
    /// Both values share the incoming object pointer. MWCC forms the more
    /// expensive second argument in f2 first, then issues the independent f1
    /// member load immediately before the call. Besides matching its latency
    /// schedule, this keeps argument evaluation from obscuring the in-place
    /// absolute-value shape.
    pub(crate) fn try_emit_member_float_abs_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [first @ Expression::Member {
            base: first_base,
            member_type: Type::Float,
            index_stride: None,
            ..
        }, second] = arguments
        else {
            return Ok(false);
        };
        let Some(second_value @ Expression::Member {
            base: second_base,
            member_type: Type::Float,
            index_stride: None,
            ..
        }) = crate::float_abs_select::abs_select_value(second)
        else {
            return Ok(false);
        };
        let (Expression::Variable(first_base), Expression::Variable(second_base)) =
            (first_base.as_ref(), second_base.as_ref())
        else {
            return Ok(false);
        };
        let both_float = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() >= 2 && types[0] == Type::Float && types[1] == Type::Float
        });
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !both_float
            || first_base != second_base
            || self
                .locations
                .get(first_base.as_str())
                .map(|location| location.register)
                != Some(Eabi::FIRST_GENERAL_ARGUMENT)
        {
            return Ok(false);
        }

        self.evaluate_float(second_value, Eabi::FIRST_FLOAT_ARGUMENT + 1)?;
        self.emit_float_abs_select(
            Eabi::FIRST_FLOAT_ARGUMENT + 1,
            Eabi::FIRST_FLOAT_ARGUMENT + 1,
            false,
        )?;
        self.evaluate_float(first, Eabi::FIRST_FLOAT_ARGUMENT)?;
        Ok(true)
    }

    /// Marshal `(object, object->float, f2, f3, object->float)` after a small
    /// forwarding wrapper has been inlined. The middle values already occupy
    /// their ABI registers; MWCC issues the independent high member load first,
    /// then the low member load, filling f4 and f1 without temporary moves.
    pub(crate) fn try_emit_interleaved_member_float_forward_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            first @ Expression::Variable(first_name),
            low @ Expression::Member {
                base: low_base,
                member_type: Type::Float,
                index_stride: None,
                ..
            },
            Expression::Variable(second_name),
            Expression::Variable(third_name),
            high @ Expression::Member {
                base: high_base,
                member_type: Type::Float,
                index_stride: None,
                ..
            },
        ] = arguments
        else {
            return Ok(false);
        };
        let (
            Expression::Variable(low_base_name),
            Expression::Variable(high_base_name),
        ) = (low_base.as_ref(), high_base.as_ref())
        else {
            return Ok(false);
        };
        let expected_types = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() >= 5
                && !matches!(types[0], Type::Float | Type::Double)
                && types[1..5].iter().all(|ty| *ty == Type::Float)
        });
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !expected_types
            || first_name != low_base_name
            || first_name != high_base_name
            || self.leaf_info(first).ok().map(|value| value.0)
                != Some(Eabi::FIRST_GENERAL_ARGUMENT)
        {
            return Ok(false);
        }

        let (Ok(second), Ok(third)) = (
            self.float_register_of(second_name),
            self.float_register_of(third_name),
        ) else {
            return Ok(false);
        };

        self.evaluate_float(high, 4)?;
        self.evaluate_float(low, Eabi::FIRST_FLOAT_ARGUMENT)?;
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 2, b: second });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 3, b: third });
        Ok(true)
    }

    /// Marshal `(object, object->float, f1, f2, f3)` without destroying the
    /// three incoming floating parameters before they shift up one ABI slot.
    ///
    /// MWCC saves f1 through f0, moves the high endpoint first, then completes
    /// the shift before loading the member into f1. The non-leaf scheduler can
    /// subsequently interleave the first three independent moves with linkage.
    pub(crate) fn try_emit_member_prefixed_float_shift_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            first @ Expression::Variable(first_name),
            member @ Expression::Member {
                base,
                member_type: Type::Float,
                index_stride: None,
                ..
            },
            Expression::Variable(second_name),
            Expression::Variable(third_name),
            Expression::Variable(fourth_name),
        ] = arguments
        else {
            return Ok(false);
        };
        let Expression::Variable(base_name) = base.as_ref() else {
            return Ok(false);
        };
        let parameter_types = self.call_parameter_types.get(name);
        let expected_types = parameter_types.is_some_and(|types| {
            types.len() >= 5
                && !matches!(types[0], Type::Float | Type::Double)
                && types[1..5]
                    .iter()
                    .all(|ty| matches!(ty, Type::Float))
        });
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !expected_types
            || first_name != base_name
            || self.leaf_info(first).ok().map(|value| value.0)
                != Some(Eabi::FIRST_GENERAL_ARGUMENT)
            || self.float_register_of(second_name).ok() != Some(1)
            || self.float_register_of(third_name).ok() != Some(2)
            || self.float_register_of(fourth_name).ok() != Some(3)
        {
            return Ok(false);
        }

        self.output
            .instructions
            .push(Instruction::FloatMove { d: 4, b: 3 });
        self.output.instructions.push(Instruction::FloatMove {
            d: FLOAT_SCRATCH,
            b: 1,
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 3, b: 2 });
        self.output.instructions.push(Instruction::FloatMove {
            d: 2,
            b: FLOAT_SCRATCH,
        });
        self.evaluate_float(member, Eabi::FIRST_FLOAT_ARGUMENT)?;
        Ok(true)
    }

    /// Marshal the two values of a terminal object-member forwarding call
    /// without first moving the shared object pointer out of r3.
    ///
    /// For `callee(&object->payload, object->length)`, the second argument has
    /// to consume the original object pointer before the first argument turns
    /// r3 into the payload address. The pre-sibling-call compiler schedules the
    /// independent load first (`lwz r4,length(r3); addi r3,r3,payload`).
    /// Keeping this beside the other argument schedules also lets the terminal
    /// wrapper owner avoid inventing a callee-saved home for `object`.
    pub(crate) fn try_emit_same_base_member_forward_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let [first, second @ Expression::Member {
            base: second_base,
            member_type,
            index_stride: None,
            ..
        }] = arguments
        else {
            return Ok(false);
        };
        let Some((first_base, _)) = direct_member_address(first) else {
            return Ok(false);
        };
        let (Expression::Variable(first_name), Expression::Variable(second_name)) =
            (first_base, second_base.as_ref())
        else {
            return Ok(false);
        };
        let word_member = matches!(
            member_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
        );
        if !direct_call
            || !word_member
            || first_name != second_name
            || self
                .locations
                .get(first_name.as_str())
                .map(|location| location.register)
                != Some(Eabi::FIRST_GENERAL_ARGUMENT)
        {
            return Ok(false);
        }

        self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        Ok(true)
    }

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
        name: &str,
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
            || matches!(
                self.call_parameter_types
                    .get(name)
                    .and_then(|types| types.get(1)),
                Some(Type::Float | Type::Double)
            )
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
