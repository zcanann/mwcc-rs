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

impl Generator {
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
    use super::global_byte_member_offset;
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
}
