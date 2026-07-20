//! EABI parameter-save areas for variadic function definitions.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit the complete empty variadic-function family.
    ///
    /// A variadic callee cannot know which register arguments its caller used,
    /// so mwcc allocates the 112-byte EABI save area even when the source body is
    /// empty. CR1 bit 6 tells it whether floating arguments were supplied: the
    /// conditional prefix saves f1..f8, while r3..r10 are always saved. Keeping
    /// this as a frame owner leaves the ordinary empty-function path truthful
    /// (`blr`) and gives non-empty variadic lowering one explicit composition
    /// point later.
    pub(crate) fn try_empty_variadic_definition(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.variadic_definition
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        self.frame_size = 112;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -112,
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
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 112,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // The conditional FPR-save block owns one source-level branch pair.
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}
