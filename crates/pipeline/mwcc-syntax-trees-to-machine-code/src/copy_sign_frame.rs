//! Generation-specific frame and bit-splice normalization for fdlibm copysign.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Function, Type};
use mwcc_versions::CopySignStyle;

impl Generator {
    pub(crate) fn schedule_copy_sign_frame(&mut self, function: &Function) {
        if function.name != "copysign"
            || function.return_type != Type::Double
            || !matches!(
                function.parameters.as_slice(),
                [first, second]
                    if first.parameter_type == Type::Double
                        && second.parameter_type == Type::Double
            )
            || !is_fused_copy_sign(&self.output.instructions)
        {
            return;
        }
        match self.behavior.copy_sign_style {
            CopySignStyle::FusedInsertion => return,
            CopySignStyle::ExplicitSignMask => {}
            CopySignStyle::ExplicitSignMaskCompactFrame => {
                self.frame_size = 24;
                self.output.instructions[0] = Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -24,
                };
                self.output.instructions[8] = Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 24,
                };
            }
        }
        crate::insert_instruction_retargeting(
            self,
            5,
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 0,
                end: 0,
            },
        );
    }
}

fn is_fused_copy_sign(instructions: &[Instruction]) -> bool {
    matches!(instructions,
        [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 },
            Instruction::StoreFloatDouble { s: 2, a: 1, offset: 16 },
            Instruction::LoadWord { d: 3, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 16 },
            Instruction::RotateAndMaskInsert { a: 0, s: 3, shift: 0, begin: 1, end: 31 },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_the_complete_fused_copy_sign_frame() {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 },
            Instruction::StoreFloatDouble { s: 2, a: 1, offset: 16 },
            Instruction::LoadWord { d: 3, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 16 },
            Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: 0,
                begin: 1,
                end: 31,
            },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ];
        assert!(is_fused_copy_sign(&instructions));

        instructions[4] = Instruction::LoadWord { d: 0, a: 1, offset: 20 };
        assert!(!is_fused_copy_sign(&instructions));
    }
}
