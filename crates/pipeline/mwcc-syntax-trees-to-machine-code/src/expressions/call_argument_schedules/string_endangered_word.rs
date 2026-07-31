//! String-address arguments that would overwrite a later incoming word.

use super::*;

fn representation_preserving_leaf(
    generator: &Generator,
    mut expression: &Expression,
) -> Option<u8> {
    loop {
        match expression {
            Expression::Cast {
                target_type:
                    Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. },
                operand,
            } => expression = operand,
            _ => {
                let (register, width, _) = generator.leaf_info(expression).ok()?;
                return (width == 32).then_some(register);
            }
        }
    }
}

impl Generator {
    /// Preserve an incoming first argument before replacing r3 with a string
    /// address for a two-argument call.
    ///
    /// MWCC splits a full-data string address around the copy:
    /// `lis r5,string@ha; mr r4,r3; addi r3,r5,string@l`. A small-data string
    /// needs only `mr r4,r3; li r3,string@sda21`. Integer/pointer casts on the
    /// second argument are representation-preserving and must not hide the
    /// register dependency.
    pub(crate) fn try_emit_string_and_endangered_word_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let [Expression::StringLiteral(bytes), second] = arguments else {
            return Ok(false);
        };
        let second_register = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        if !direct_call
            || representation_preserving_leaf(self, second)
                .is_none_or(|source| source != Eabi::FIRST_GENERAL_ARGUMENT)
        {
            return Ok(false);
        }

        let small_data_string = !self.behavior.string_literals_packed
            && self.behavior.global_addressing == GlobalAddressing::SmallData
            && bytes.len() + 1 <= 8;
        if small_data_string {
            self.output.instructions.push(Instruction::move_register(
                second_register,
                Eabi::FIRST_GENERAL_ARGUMENT,
            ));
            self.emit_string_literal(bytes, Eabi::FIRST_GENERAL_ARGUMENT)?;
            return Ok(true);
        }

        let placeholder = self.string_literal_placeholder(bytes);
        if self.behavior.string_literals_packed {
            self.output.packed_string_literals = true;
        }
        let high = second_register + 1;
        self.emit_address_high(high, &placeholder);
        self.output.instructions.push(Instruction::move_register(
            second_register,
            Eabi::FIRST_GENERAL_ARGUMENT,
        ));
        self.emit_string_address_low(&placeholder, high, Eabi::FIRST_GENERAL_ARGUMENT);
        Ok(true)
    }
}
