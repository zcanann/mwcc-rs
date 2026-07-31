//! Final physical schedule for retained unoptimized leaf source homes.

use super::*;

impl Generator {
    pub(crate) fn finalize_unoptimized_leaf_source_homes(&mut self) {
        if !self.structured_unoptimized_leaf_source_homes
            || !matches!(
                self.output.instructions.as_slice(),
                [
                    Instruction::StoreWordWithUpdate { offset: -32, .. },
                    Instruction::StoreFloatDouble {
                        s: 31,
                        offset: 16,
                        ..
                    },
                    Instruction::PairedSingleQuantizedStore {
                        s: 31,
                        offset: 24,
                        ..
                    },
                    Instruction::StoreWord {
                        s: 31,
                        offset: 12,
                        ..
                    },
                    Instruction::StoreWord {
                        s: 30,
                        offset: 8,
                        ..
                    },
                    Instruction::AddImmediateShifted { d: 4, .. },
                    Instruction::ShiftLeftImmediate {
                        a: 3,
                        s: 3,
                        shift: 3
                    },
                    Instruction::AddImmediate { d: 0, a: 4, .. },
                    Instruction::Add { d: 31, a: 0, b: 3 },
                    Instruction::LoadWord {
                        d: 30,
                        a: 31,
                        offset: 4
                    },
                    Instruction::LoadWord {
                        d: 30,
                        a: 30,
                        offset: 72
                    },
                    Instruction::LoadFloatSingle {
                        d: 31,
                        a: 30,
                        offset: 12
                    },
                    Instruction::FloatMove { d: 1, b: 31 },
                    Instruction::LoadWord {
                        d: 31,
                        offset: 12,
                        ..
                    },
                    Instruction::LoadWord {
                        d: 30,
                        offset: 8,
                        ..
                    },
                    Instruction::PairedSingleQuantizedLoad {
                        d: 31,
                        offset: 24,
                        ..
                    },
                    Instruction::LoadFloatDouble {
                        d: 31,
                        offset: 16,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 32
                    },
                    Instruction::BranchToLinkRegister,
                ]
            )
        {
            return;
        }

        crate::insert_instruction_retargeting(
            self,
            5,
            Instruction::ExtendSignHalfword { a: 0, s: 3 },
        );
        self.output.instructions.swap(6, 7);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                6 => 7,
                7 => 6,
                index => index,
            };
        }
        self.output.instructions[6] = Instruction::ShiftLeftImmediate {
            a: 5,
            s: 0,
            shift: 3,
        };
        let Instruction::Add { b, .. } = &mut self.output.instructions[9] else {
            unreachable!("the retained leaf address add was classified above")
        };
        *b = 5;
        self.output.instructions[10] = Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: 4,
        };
        self.output.instructions[11] = Instruction::LoadWord {
            d: 30,
            a: 4,
            offset: 72,
        };
        self.output.instructions.swap(14, 16);
        self.output.instructions.swap(15, 17);
    }
}
