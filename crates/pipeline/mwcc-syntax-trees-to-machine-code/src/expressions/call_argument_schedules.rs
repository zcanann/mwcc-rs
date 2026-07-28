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

fn stable_call_binary(
    expression: &Expression,
) -> Option<(BinaryOperator, &Expression, &Expression, bool)> {
    let Expression::Binary {
        operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract),
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (stable, call @ Expression::Call { .. }) => Some((*operator, stable, call, false)),
        (call @ Expression::Call { .. }, stable) => Some((*operator, stable, call, true)),
        _ => None,
    }
}

impl Generator {
    /// Marshal `(i16, large_global_array, i16)` with the array's high half in
    /// the LR-store latency window.
    ///
    /// The ordinary global-plus-constant owner models scalar loads. An array
    /// argument is its address, so it needs the distinct `lis`/`addi` chain:
    /// `lis r4,array; li r3,a; [store LR]; addi r4; li r5,b`.
    pub(crate) fn try_emit_constant_global_array_constant_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [
            Expression::IntegerLiteral(first),
            Expression::Variable(array),
            Expression::IntegerLiteral(third),
        ] = arguments
        else {
            return Ok(false);
        };
        let direct_call = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let Some(&array_size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        let (Ok(first), Ok(third)) = (i16::try_from(*first), i16::try_from(*third)) else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && array_size <= 8)
        {
            return Ok(false);
        }

        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        self.emit_address_high(first_argument + 1, array);
        self.output
            .instructions
            .push(Instruction::load_immediate(first_argument, first));
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first_argument + 1,
            a: first_argument + 1,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(first_argument + 2, third));
        Ok(true)
    }

    /// Marshal `(global_array, packed_string, call_expression)` around the
    /// nested call's result in r3.
    ///
    /// MWCC evaluates the call-bearing third argument first, then overlaps the
    /// two absolute-address dependency chains before copying the nested result
    /// to r5. The global-array high half uses r6 so r3 remains available until
    /// the copy:
    ///
    /// ```text
    /// bl nested
    /// lis r4,string@ha
    /// lis r6,array@ha
    /// addi r4,r4,string@l
    /// mr r5,r3
    /// addi r3,r6,array@l
    /// ```
    ///
    /// This avoids a callee-saved register because both prefix arguments are
    /// reloadable after the nested call.
    pub(crate) fn try_emit_global_array_string_nested_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [
            Expression::Variable(array),
            Expression::StringLiteral(string),
            nested,
        ] = arguments
        else {
            return Ok(false);
        };
        let direct_outer = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let Some(&array_size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if !direct_outer
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && array_size <= 8)
            || !self.behavior.string_literals_packed
            || !self.behavior.schedule_latency_slots
            || self.is_float_value(nested)
            || !expression_has_call(nested)
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        self.evaluate_general(nested, first)?;

        self.output.packed_string_literals = true;
        let string = self.string_literal_placeholder(string);
        self.emit_address_high(first + 1, &string);
        self.emit_address_high(first + 3, array);
        self.emit_string_address_low(&string, first + 1, first + 1);
        self.emit_integer_materialization_copy(first + 2, first);
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first,
            a: first + 3,
            immediate: 0,
        });
        Ok(true)
    }

    /// Marshal a reloadable general prefix after evaluating a call-bearing
    /// floating tail argument.
    ///
    /// The nested call owns r3-r12 but returns through the independent FPR
    /// sequence. A frame address, global, or nonvolatile-register expression
    /// can therefore be reconstructed in r3 after the complete float
    /// expression reaches f1. Aggregate-returning calls may prepend their
    /// hidden-result address to the source signature, producing
    /// `(result*, this*, float)`; that ABI-only prefix follows the same rule.
    pub(crate) fn try_emit_reloadable_general_prefix_call_bearing_float_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let Some((tail, prefix)) = arguments.split_last() else {
            return Ok(false);
        };
        if prefix.is_empty() {
            return Ok(false);
        }
        let direct_call = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let hidden_result = matches!(
            self.call_return_types.get(name),
            Some(Type::Struct { .. })
        );
        let expected_types = self.call_parameter_types.get(name).is_some_and(|types| {
            let source_index = |abi_index: usize| {
                abi_index.checked_sub(usize::from(hidden_result))
            };
            arguments.len() == types.len() + usize::from(hidden_result)
                && prefix.iter().enumerate().all(|(index, _)| {
                    hidden_result && index == 0
                        || source_index(index)
                            .and_then(|index| types.get(index))
                            .is_some_and(|ty| !matches!(ty, Type::Float | Type::Double))
                })
                && source_index(arguments.len() - 1)
                    .and_then(|index| types.get(index))
                    .is_some_and(|ty| matches!(ty, Type::Float | Type::Double))
        });
        let prefix_is_reloadable = prefix.iter().all(|argument| {
            !expression_has_call(argument)
                && self
                    .registers_used_by(argument)
                    .into_iter()
                    .all(|register| !matches!(register, 0 | 3..=12))
        });
        if !direct_call
            || !expected_types
            || !prefix_is_reloadable
            || !self.is_float_value(tail)
            || !expression_has_call(tail)
        {
            return Ok(false);
        }

        self.evaluate_float(tail, Eabi::FIRST_FLOAT_ARGUMENT)
            .map_err(|mut diagnostic| {
                diagnostic.message.push_str(&format!(
                    " (while scheduling call-bearing float tail argument to '{name}')"
                ));
                diagnostic
            })?;
        for (index, argument) in prefix.iter().enumerate() {
            self.evaluate_general(
                argument,
                Eabi::FIRST_GENERAL_ARGUMENT + index as u8,
            )
            .map_err(|mut diagnostic| {
                diagnostic.message.push_str(&format!(
                    " (while reconstructing general-prefix argument {index} to '{name}')"
                ));
                diagnostic
            })?;
        }
        Ok(true)
    }

    /// Marshal one argument-free nested call in a later general-class slot
    /// before its reloadable siblings.
    ///
    /// For `allocate(saved_count * 16, heap(), 0)`, the inner result first
    /// lands in r3 and is copied to r4. The saved count and literal are then
    /// reconstructed directly in r3 and r5. This is safe only when every
    /// sibling reads constants, globals, or nonvolatile registers.
    pub(crate) fn try_emit_zero_arg_nested_general_argument(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let direct_outer = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let all_general = self.call_parameter_types.get(name).map_or_else(
            || arguments.iter().all(|argument| !self.is_float_value(argument)),
            |types| {
                types.len() >= arguments.len()
                    && types[..arguments.len()]
                        .iter()
                        .all(|ty| !matches!(ty, Type::Float | Type::Double))
            },
        );
        if !direct_outer || !all_general {
            return Ok(false);
        }
        let mut nested = arguments.iter().enumerate().filter_map(|(index, argument)| {
            let Expression::Call {
                name: nested_name,
                arguments: nested_arguments,
            } = argument
            else {
                return None;
            };
            (index > 0
                && nested_arguments.is_empty()
                && !self.globals.contains_key(nested_name)
                && !self.locations.contains_key(nested_name)
                && !self.known_locals.contains(nested_name)
                && !self.is_float_value(argument))
            .then_some((index, argument))
        });
        let Some((nested_index, nested_argument)) = nested.next() else {
            return Ok(false);
        };
        if nested.next().is_some()
            || arguments
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != nested_index)
                .any(|(_, argument)| {
                    expression_has_call(argument)
                        || self
                            .registers_used_by(argument)
                            .into_iter()
                            .any(|register| matches!(register, 0 | 3..=12))
                })
        {
            return Ok(false);
        }

        self.evaluate_general(nested_argument, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.emit_integer_materialization_copy(
            Eabi::FIRST_GENERAL_ARGUMENT + nested_index as u8,
            Eabi::FIRST_GENERAL_ARGUMENT,
        );
        for (index, argument) in arguments.iter().enumerate() {
            if index != nested_index {
                self.evaluate_general(
                    argument,
                    Eabi::FIRST_GENERAL_ARGUMENT + index as u8,
                )?;
            }
        }
        Ok(true)
    }

    /// Marshal a general-class nested second argument before a reloadable first
    /// argument. Every register read by the first expression must survive the
    /// nested call; afterward the first value can be reconstructed directly in
    /// r3 while the complete nested expression is formed in r4.
    ///
    /// This covers both saved leaves (`registerState(this, new State)`) and
    /// saved-base member reads (`set(p->field, saved + get(p->field))`).
    pub(crate) fn try_emit_reloadable_first_nested_second_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [first, second] = arguments else {
            return Ok(false);
        };
        let direct_call = !self.globals.contains_key(name) && !self.locations.contains_key(name);
        let both_general = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() >= 2
                && !matches!(types[0], Type::Float | Type::Double)
                && !matches!(types[1], Type::Float | Type::Double)
        });
        let first_is_reloadable = !expression_has_call(first)
            && self
                .registers_used_by(first)
                .into_iter()
                .all(|register| !matches!(register, 0 | 3..=12));
        if !direct_call
            || !both_general
            || !first_is_reloadable
            || !expression_has_call(second)
        {
            return Ok(false);
        }

        if let Some((operator, stable, nested_call, call_is_left)) = stable_call_binary(second) {
            let Some(stable_register) = leaf_name(stable).and_then(|name| self.lookup_general(name))
            else {
                return Ok(false);
            };
            if stable_register < 14 || self.is_float_value(nested_call) {
                return Ok(false);
            }
            self.evaluate_general(nested_call, GENERAL_SCRATCH)?;
            self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
            let (left, right) = if call_is_left {
                (GENERAL_SCRATCH, stable_register)
            } else {
                (stable_register, GENERAL_SCRATCH)
            };
            self.output.instructions.push(match operator {
                BinaryOperator::Add => Instruction::Add {
                    d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    a: left,
                    b: right,
                },
                BinaryOperator::Subtract => Instruction::SubtractFrom {
                    d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    a: right,
                    b: left,
                },
                _ => unreachable!(),
            });
            return Ok(true);
        }

        self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        Ok(true)
    }

    /// Marshal `(saved_object, i16, saved_float)` with the FPR copy first.
    /// Both saved values survive any preceding call. MWCC exposes the float
    /// move earliest, then materializes the GPR arguments in ABI order.
    pub(crate) fn try_emit_saved_float_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            Expression::Variable(first_name),
            second @ Expression::IntegerLiteral(value),
            third @ Expression::Variable(third_name),
        ] = arguments
        else {
            return Ok(false);
        };
        let expected_types = self.call_parameter_types.get(name).is_some_and(|types| {
            types.len() >= 3
                && !matches!(types[0], Type::Float | Type::Double)
                && !matches!(types[1], Type::Float | Type::Double)
                && matches!(types[2], Type::Float | Type::Double)
        });
        let Some(first_source) = self.lookup_general(first_name) else {
            return Ok(false);
        };
        let Ok(third_source) = self.float_register_of(third_name) else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !expected_types
            || first_source < 14
            || third_source < 14
            || !(i16::MIN as i64..=i16::MAX as i64).contains(value)
        {
            return Ok(false);
        }

        self.evaluate_float(third, Eabi::FIRST_FLOAT_ARGUMENT)?;
        self.emit_integer_materialization_copy(Eabi::FIRST_GENERAL_ARGUMENT, first_source);
        self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        Ok(true)
    }

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
        let (Expression::Variable(first_name), Expression::Variable(_)) =
            (first_base.as_ref(), second_base)
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
        let Some(first_base_register) = self.lookup_general(first_name) else {
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
        let expected_types = self.call_parameter_types.get(name).is_some_and(|types| {
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

    /// Schedule `(small_string, i16, large_string)` through the first argument
    /// register. Build 163 forms the large third argument in r3/r5 before its
    /// packed small-data first argument overwrites r3, exposing both address
    /// halves ahead of the cheap integer line materialization.
    pub(crate) fn try_emit_mixed_string_line_arguments(
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
            || first.len() + 1 > 8
            || third.len() + 1 <= 8
            || !(i16::MIN as i64..=i16::MAX as i64).contains(line)
        {
            return Ok(false);
        }

        let third = self.string_literal_placeholder(third);
        self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT, &third);
        self.emit_string_address_low(
            &third,
            Eabi::FIRST_GENERAL_ARGUMENT,
            Eabi::FIRST_GENERAL_ARGUMENT + 2,
        );
        self.evaluate_general(&arguments[0], Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.output.instructions.push(Instruction::load_immediate(
            Eabi::FIRST_GENERAL_ARGUMENT + 1,
            *line as i16,
        ));
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

    /// Schedule `(saved, word_global, i16)` with the literal ahead of the load.
    ///
    /// The early Dolphin `memcpy(id, idTmp, 32)` family keeps `id` in a
    /// callee-saved register. MWCC forwards that value first, materializes the
    /// independent size next, and leaves the global pointer load immediately
    /// before the call. The structured statement scheduler may subsequently
    /// lift the first two independent instructions across preceding stores.
    pub(crate) fn try_emit_saved_global_constant_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let [
            first @ Expression::Variable(saved),
            second @ Expression::Variable(global),
            third @ Expression::IntegerLiteral(value),
        ] = arguments
        else {
            return Ok(false);
        };
        let Some(saved_register) = self.lookup_general(saved) else {
            return Ok(false);
        };
        let all_general = self.call_parameter_types.get(name).is_none_or(|types| {
            types.len() >= 3
                && types[..3]
                    .iter()
                    .all(|ty| !matches!(ty, Type::Float | Type::Double))
        });
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !all_general
            || saved_register < 14
            || !self.globals.contains_key(global.as_str())
            || !(i16::MIN as i64..=i16::MAX as i64).contains(value)
        {
            return Ok(false);
        }

        self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.evaluate_general(third, Eabi::FIRST_GENERAL_ARGUMENT + 2)?;
        self.evaluate_general(second, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
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
