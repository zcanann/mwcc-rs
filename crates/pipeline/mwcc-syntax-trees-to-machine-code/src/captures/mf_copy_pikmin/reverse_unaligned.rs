//! Measured rev unaligned worker schedule.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;

impl Generator {
    pub(super) fn emit_pikmin_copy_rev_unaligned(&mut self) {
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 10, 17, 36, 39] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Add { d: 12, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 12,
                clear: 0x1e,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 11, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::move_register(3, 0));
        self.emit_branch_conditional_to(12, 2, labels[&10]); // beq
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 3, b: 5 });
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 11,
                offset: -1,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 3,
                a: 3,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 12,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 11,
            shift: 3,
            begin: 0x1b,
            end: 0x1c,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 9,
                s: 11,
                clear: 0x1e,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 10,
                a: 4,
                immediate: 0x20,
            });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: 9,
                immediate: 4,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 11, a: 11, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 7,
                a: 11,
                offset: -4,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 6,
                s: 5,
                shift: 3,
            });
        self.bind_label(labels[&17]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 11,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 7, b: 10 });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 8, b: 4 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 12,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 8, b: 10 });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 7,
                a: 11,
                offset: -8,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 7, b: 4 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 12,
                offset: -8,
            });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 5,
                shift: 0,
                begin: 0x1d,
                end: 0x1d,
            });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 3,
                a: 11,
                offset: -4,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightWord { a: 0, s: 7, b: 10 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 3, s: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 12,
                offset: -4,
            });
        self.bind_label(labels[&36]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 5,
                s: 5,
                clear: 0x1e,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 11, a: 11, b: 9 });
        self.bind_label(labels[&39]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 11,
                offset: -1,
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
                a: 12,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&39]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}
