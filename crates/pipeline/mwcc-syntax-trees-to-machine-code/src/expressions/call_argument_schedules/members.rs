//! Direct-call schedules for scalar members loaded through global aggregates.

#[allow(unused_imports)]
use super::*;

fn global_byte_member_offset(
    expression: &Expression,
) -> Option<(&str, u32, i64)> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let (member, adjustment) = if let Some(adjustment) = constant_value(left) {
        (right.as_ref(), adjustment)
    } else {
        (left.as_ref(), constant_value(right)?)
    };
    let Expression::Member {
        base,
        offset,
        member_type: Type::UnsignedChar,
        index_stride: None,
    } = member
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, *offset, adjustment))
}

fn global_word_member(expression: &Expression) -> Option<(&str, u32)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Int | Type::UnsignedInt,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, *offset))
}

#[derive(Debug, PartialEq)]
struct SharedGlobalPointerMembers<'a> {
    global: &'a str,
    first_offset: u32,
    first_type: Type,
    second_offset: u32,
    second_type: Type,
}

fn shared_global_pointer_members(
    arguments: &[Expression],
) -> Option<SharedGlobalPointerMembers<'_>> {
    let [
        Expression::Member {
            base: first_base,
            offset: first_offset,
            member_type: first_type,
            index_stride: None,
        },
        Expression::Member {
            base: second_base,
            offset: second_offset,
            member_type: second_type,
            index_stride: None,
        },
    ] = arguments
    else {
        return None;
    };
    let (Expression::Variable(first_global), Expression::Variable(second_global)) =
        (first_base.as_ref(), second_base.as_ref())
    else {
        return None;
    };
    if first_global != second_global
        || first_type.width() != 32
        || second_type.width() != 32
        || matches!(first_type, Type::Float)
        || matches!(second_type, Type::Float)
    {
        return None;
    }
    Some(SharedGlobalPointerMembers {
        global: first_global,
        first_offset: *first_offset,
        first_type: *first_type,
        second_offset: *second_offset,
        second_type: *second_type,
    })
}

impl Generator {
    /// Marshal two word-sized members through one global struct pointer.
    ///
    /// Loading each argument independently reloads the global pointer. MWCC
    /// keeps that pointer in r4, loads the first member into r3, and consumes
    /// the same r4 base while replacing it with the second argument:
    /// `lwz r4,g@sda21(r0); lwz r3,a(r4); lwz r4,b(r4)`.
    pub(crate) fn try_emit_shared_global_pointer_member_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let Some(plan) = shared_global_pointer_members(arguments) else {
            return Ok(false);
        };
        let direct_call = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        if !direct_call
            || self.volatile_globals.contains(plan.global)
            || !matches!(
                self.globals.get(plan.global),
                Some(Type::StructPointer { .. })
            )
        {
            return Ok(false);
        }
        let (Ok(first_offset), Ok(second_offset)) = (
            i16::try_from(plan.first_offset),
            i16::try_from(plan.second_offset),
        ) else {
            return Ok(false);
        };
        let first_pointee = pointee_of_type(plan.first_type)
            .ok_or_else(|| Diagnostic::error("shared pointer member has no load width"))?;
        let second_pointee = pointee_of_type(plan.second_type)
            .ok_or_else(|| Diagnostic::error("shared pointer member has no load width"))?;
        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        let shared_base = first_argument + 1;
        self.emit_global_load_value(plan.global, shared_base)?;
        self.output.instructions.push(displacement_load(
            first_pointee,
            first_argument,
            shared_base,
            first_offset,
        )?);
        self.output.instructions.push(displacement_load(
            second_pointee,
            shared_base,
            shared_base,
            second_offset,
        )?);
        Ok(true)
    }

    /// Marshal `(global_array, packed_string, global.word)` using r4 first as
    /// the aggregate address and then as the final format-string argument.
    ///
    /// All three absolute addresses overlap: global and array highs fill the
    /// linkage-save window, their lows publish r4/r3, and the string uses r6
    /// while the word load consumes the temporary aggregate address.
    pub(crate) fn try_emit_global_array_string_global_word_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [Expression::Variable(array), Expression::StringLiteral(string), value] = arguments
        else {
            return Ok(false);
        };
        let Some((global, member_offset)) = global_word_member(value) else {
            return Ok(false);
        };
        let Ok(member_offset) = i16::try_from(member_offset) else {
            return Ok(false);
        };
        let direct = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let Some(&array_size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if !direct
            || !self.addressable_globals.contains_key(global)
            || array_size <= 8
            || !self.behavior.string_literals_packed
            || !self.behavior.schedule_latency_slots
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        self.output.packed_string_literals = true;
        let string = self.string_literal_placeholder(string);
        self.emit_address_high(first, global);
        self.emit_address_high(first + 2, array);
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first + 1,
            a: first,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first,
            a: first + 2,
            immediate: 0,
        });
        self.emit_address_high(first + 3, &string);
        self.output.instructions.push(Instruction::LoadWord {
            d: first + 2,
            a: first + 1,
            offset: member_offset,
        });
        self.emit_string_address_low(&string, first + 3, first + 1);
        Ok(true)
    }

    /// Marshal `(global_array, packed_string, global.byte + i16)` with three
    /// independent address chains occupying MWCC's measured dependency slots.
    ///
    /// The aggregate address starts in r3, the packed format in r4, and the
    /// destination array uses r6 so the member can load into r5 before r3 is
    /// published:
    ///
    /// ```text
    /// lis r3,global
    /// lis r4,string
    /// addi r3,r3,global
    /// lis r6,array
    /// addi r4,r4,string
    /// lbz r5,member(r3)
    /// addi r3,r6,array
    /// addi r5,r5,adjustment
    /// ```
    pub(crate) fn try_emit_global_array_string_global_byte_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [Expression::Variable(array), Expression::StringLiteral(string), value] = arguments
        else {
            return Ok(false);
        };
        let Some((global, member_offset, adjustment)) =
            global_byte_member_offset(value)
        else {
            return Ok(false);
        };
        let (Ok(member_offset), Ok(adjustment)) =
            (i16::try_from(member_offset), i16::try_from(adjustment))
        else {
            return Ok(false);
        };
        let direct = !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name);
        let Some(&array_size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if !direct
            || !self.addressable_globals.contains_key(global)
            || array_size <= 8
            || !self.behavior.string_literals_packed
            || !self.behavior.schedule_latency_slots
        {
            return Ok(false);
        }

        let first = Eabi::FIRST_GENERAL_ARGUMENT;
        self.output.packed_string_literals = true;
        let string = self.string_literal_placeholder(string);
        self.emit_address_high(first, global);
        self.emit_address_high(first + 1, &string);
        self.emit_address_low(first, global);
        self.emit_address_high(first + 3, array);
        self.emit_string_address_low(&string, first + 1, first + 1);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: first + 2,
            a: first,
            offset: member_offset,
        });
        self.record_relocation(RelocationKind::Addr16Lo, array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: first,
            a: first + 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: first + 2,
            a: first + 2,
            immediate: adjustment,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        global_byte_member_offset, global_word_member, shared_global_pointer_members,
        SharedGlobalPointerMembers,
    };
    use mwcc_syntax_trees::{BinaryOperator, Expression, Type};

    #[test]
    fn recognizes_a_commuted_global_byte_adjustment() {
        let expression = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::IntegerLiteral(1)),
            right: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("globals".into())),
                offset: 1745,
                member_type: Type::UnsignedChar,
                index_stride: None,
            }),
        };

        assert_eq!(
            global_byte_member_offset(&expression),
            Some(("globals", 1745, 1))
        );
    }

    #[test]
    fn recognizes_a_global_word_member() {
        let expression = Expression::Member {
            base: Box::new(Expression::Variable("globals".into())),
            offset: 7104,
            member_type: Type::UnsignedInt,
            index_stride: None,
        };

        assert_eq!(global_word_member(&expression), Some(("globals", 7104)));
    }

    #[test]
    fn recognizes_two_members_of_one_global_pointer() {
        let member = |offset, member_type| Expression::Member {
            base: Box::new(Expression::Variable("boot_info".into())),
            offset,
            member_type,
            index_stride: None,
        };
        let arguments = vec![
            member(56, Type::Pointer(mwcc_syntax_trees::Pointee::Int)),
            member(60, Type::UnsignedInt),
        ];

        assert_eq!(
            shared_global_pointer_members(&arguments),
            Some(SharedGlobalPointerMembers {
                global: "boot_info",
                first_offset: 56,
                first_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                second_offset: 60,
                second_type: Type::UnsignedInt,
            })
        );
    }
}
