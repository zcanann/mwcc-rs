//! Measured rev aligned worker schedule.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;

impl Generator {
    pub(super) fn emit_pikmin_copy_rev_aligned(&mut self) {
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 10, 12, 30, 32, 36, 38] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 3, b: 5 });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 0x1e,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 5 });
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
                a: 4,
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
                a: 6,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.bind_label(labels[&10]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 5,
                shift: 0x1b,
                begin: 5,
                end: 0x1f,
            });
        self.emit_branch_conditional_to(12, 2, labels[&30]); // beq
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -4,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 3,
                a: 3,
                immediate: -1,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -0xc,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -0xc,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -0x10,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -0x10,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -0x14,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -0x14,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -0x18,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -0x18,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: -0x1c,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: -0x1c,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 4,
                offset: -0x20,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 6,
                offset: -0x20,
            });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.bind_label(labels[&30]);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskRecord {
                a: 3,
                s: 5,
                shift: 0x1e,
                begin: 0x1d,
                end: 0x1f,
            });
        self.emit_branch_conditional_to(12, 2, labels[&36]); // beq
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: 0,
                a: 4,
                offset: -4,
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
            .push(Instruction::StoreWordWithUpdate {
                s: 0,
                a: 6,
                offset: -4,
            });
        self.emit_branch_conditional_to(4, 2, labels[&32]); // bne
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
        self.bind_label(labels[&38]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
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
                a: 6,
                offset: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}
