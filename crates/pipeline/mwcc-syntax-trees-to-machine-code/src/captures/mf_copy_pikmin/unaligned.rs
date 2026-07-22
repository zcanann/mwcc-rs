//! Measured unaligned worker schedule.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;

impl Generator {
    pub(super) fn emit_pikmin_copy_unaligned(&mut self) {
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 10, 19, 38, 44] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Negate { d: 0, a: 3 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 6,
                s: 0,
                clear: 0x1e,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.emit_branch_conditional_to(12, 2, labels[&10]); // beq
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 6, b: 5 });
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 3,
                offset: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 11,
                s: 0,
                clear: 0x1e,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 11, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 4,
            immediate: -3,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 9,
                a: 8,
                offset: 4,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 0,
            shift: 3,
            begin: 0x1b,
            end: 0x1c,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 12,
                a: 4,
                immediate: 0x20,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: -3,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 7,
                s: 5,
                shift: 3,
            });
        self.bind_label(labels[&19]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 8,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 9, b: 4 });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 7,
                a: 7,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 10, b: 12 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 10, b: 4 });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 9,
                a: 8,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 9, b: 12 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 6,
                offset: 8,
            });
        self.emit_branch_conditional_to(4, 2, labels[&19]); // bne
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 5,
                shift: 0,
                begin: 0x1d,
                end: 0x1d,
            });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 8,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 9, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 0, b: 12 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 6,
                offset: 4,
            });
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 5,
                s: 5,
                clear: 0x1e,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 8,
            immediate: 3,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 3,
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: 11,
                immediate: 4,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 4, a: 0, b: 4 });
        self.bind_label(labels[&44]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 3,
                offset: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&44]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}
