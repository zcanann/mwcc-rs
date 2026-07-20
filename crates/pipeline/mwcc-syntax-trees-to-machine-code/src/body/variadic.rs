//! EABI parameter-save areas for variadic function definitions.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit the complete side-effect-free variadic-function family.
    ///
    /// A variadic callee cannot know which register arguments its caller used,
    /// so mwcc allocates an EABI save area even when the source body is
    /// side-effect free. CR1 bit 6 tells it whether floating arguments were
    /// supplied: the
    /// conditional prefix saves f1..f8, while r3..r10 are always saved. Keeping
    /// this as a frame owner leaves the ordinary non-variadic empty-function
    /// path truthful (`blr`) and gives body-bearing variadic lowering one
    /// explicit composition point later.
    pub(crate) fn try_simple_variadic_definition(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.variadic_definition
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || self.entry_parameter_words > 8
        {
            return Ok(false);
        }
        let return_constant = match (&function.return_type, &function.return_expression) {
            (Type::Void, None) => None,
            (
                Type::Int | Type::UnsignedInt | Type::Short | Type::UnsignedShort | Type::Char
                | Type::UnsignedChar,
                Some(expression),
            ) => constant_value(expression).and_then(|value| i16::try_from(value).ok()),
            _ => return Ok(false),
        };
        if function.return_type != Type::Void && return_constant.is_none() {
            return Ok(false);
        }

        self.frame_size = (104 + 4 * self.entry_parameter_words as i16 + 7) & !7;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            });
        let skip_float_saves = self.fresh_label();
        // `bne cr1,...`: BO=4, BI=6. The caller clears this bit when no
        // floating arguments occupy f1..f8.
        self.emit_branch_conditional_to(4, 6, skip_float_saves);
        for register in 1..=8 {
            self.output.instructions.push(Instruction::StoreFloatDouble {
                s: register,
                a: 1,
                offset: 32 + i16::from(register) * 8,
            });
        }
        self.bind_label(skip_float_saves);
        for register in 3..=10 {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset: -4 + i16::from(register) * 4,
            });
            // The return register becomes dead as an incoming argument after
            // its save. MWCC fills that exact slot before saving r4..r10.
            if register == 3 {
                if let Some(value) = return_constant {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, value));
                }
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // The conditional FPR-save block owns one source-level branch pair.
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}
