//! Direct-call schedules that compose an inner call with reloadable arguments.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Marshal `(global_array, packed_string, zero_arg_call + i16)` while
    /// preserving the nested result until the final argument is formed.
    ///
    /// MWCC evaluates the nested call first, starts both reloadable address
    /// chains, then saves the result in r6 before constructing r3 and r5:
    ///
    /// ```text
    /// bl nested
    /// lis r4,string@ha
    /// lis r5,array@ha
    /// mr r6,r3
    /// addi r4,r4,string@l
    /// addi r3,r5,array@l
    /// addi r5,r6,offset
    /// ```
    pub(crate) fn try_emit_global_array_string_call_offset_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [
            Expression::Variable(array),
            Expression::StringLiteral(string),
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            },
        ] = arguments
        else {
            return Ok(false);
        };
        let (nested, offset) = match (left.as_ref(), right.as_ref()) {
            (nested @ Expression::Call { .. }, Expression::IntegerLiteral(offset))
            | (Expression::IntegerLiteral(offset), nested @ Expression::Call { .. }) => {
                (nested, *offset)
            }
            _ => return Ok(false),
        };
        let Expression::Call {
            name: nested_name,
            arguments: nested_arguments,
        } = nested
        else {
            return Ok(false);
        };
        let direct = |callee: &str| {
            !self.globals.contains_key(callee)
                && !self.locations.contains_key(callee)
                && !self.known_locals.contains(callee)
        };
        let Some(&array_size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        let Ok(offset) = i16::try_from(offset) else {
            return Ok(false);
        };
        if !direct(name)
            || !direct(nested_name)
            || !nested_arguments.is_empty()
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && array_size <= 8)
            || !self.behavior.string_literals_packed
            || !self.behavior.schedule_latency_slots
            || self.is_float_value(nested)
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        self.emit_call(nested_name, nested_arguments, None, false)?;

        self.output.packed_string_literals = true;
        let string = self.string_literal_placeholder(string);
        self.emit_address_high(first + 1, &string);
        self.emit_address_high(first + 2, array);
        self.emit_integer_materialization_copy(first + 3, first);
        self.emit_string_address_low(&string, first + 1, first + 1);
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first,
            a: first + 2,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: first + 2,
            a: first + 3,
            immediate: offset,
        });
        Ok(true)
    }

    /// Marshal `(global_struct.pointer.word, i16)` with the literal in the
    /// global address's dependency slot.
    ///
    /// A chained member read first forms the aggregate address and then
    /// performs two dependent word loads. MWCC issues the independent `li r4`
    /// between the address high and low halves:
    /// `lis r3,global; li r4,k; addi r3,r3,global; lwz r3,outer(r3);
    /// lwz r3,inner(r3)`.
    pub(crate) fn try_emit_global_chained_member_constant_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [Expression::Member {
            base,
            offset: inner_offset,
            member_type,
            index_stride: None,
        }, Expression::IntegerLiteral(value)] = arguments
        else {
            return Ok(false);
        };
        let Expression::Member {
            base,
            offset: outer_offset,
            member_type: Type::StructPointer { .. } | Type::Pointer(_),
            index_stride: None,
        } = base.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(global) = base.as_ref() else {
            return Ok(false);
        };
        let direct_call = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let word_member = matches!(
            member_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
        );
        let (Ok(value), Ok(outer_offset), Ok(inner_offset)) = (
            i16::try_from(*value),
            i16::try_from(*outer_offset),
            i16::try_from(*inner_offset),
        ) else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !word_member
            || !matches!(
                self.addressable_globals.get(global.as_str()),
                Some(Type::Struct { .. })
            )
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        self.emit_address_high(first, global);
        self.output
            .instructions
            .push(Instruction::load_immediate(first + 1, value));
        self.emit_address_low(first, global);
        self.output.instructions.push(Instruction::LoadWord {
            d: first,
            a: first,
            offset: outer_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: first,
            a: first,
            offset: inner_offset,
        });
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
        let [Expression::Variable(array), Expression::StringLiteral(string), nested] = arguments
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

    /// Marshal `(frame_array, packed_string, call_expression)` after evaluating
    /// the nested third argument.
    ///
    /// The frame address and packed string are both reloadable after the inner
    /// call. MWCC starts the string address, copies the result to r5, completes
    /// the string in r4, then forms the frame address in r3.
    pub(crate) fn try_emit_frame_array_string_nested_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [frame_array @ Expression::Variable(array), Expression::StringLiteral(string), nested] =
            arguments
        else {
            return Ok(false);
        };
        let direct_outer = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let frame_array_is_address = self
            .frame_slots
            .get(array.as_str())
            .is_some_and(|slot| slot.is_array);
        if !direct_outer
            || !frame_array_is_address
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
        self.emit_integer_materialization_copy(first + 2, first);
        self.emit_string_address_low(&string, first + 1, first + 1);
        self.evaluate_general(frame_array, first)?;
        Ok(true)
    }
}
