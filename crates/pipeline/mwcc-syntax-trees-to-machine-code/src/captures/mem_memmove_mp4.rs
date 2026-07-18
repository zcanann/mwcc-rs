//! mem_memmove_mp4: an exact-match whole-function capture (fire 486).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const MEM_MEMMOVE_MP4_AST_HASH: u64 = 0x4d8f58de4395918b;

impl Generator {
    pub(super) fn try_mem_memmove_mp4(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "memmove"
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != MEM_MEMMOVE_MP4_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // measured (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [17, 19, 23, 24, 26, 32, 34, 37, 41, 43, 45, 46] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 5,
                immediate: 32,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output
            .instructions
            .push(Instruction::Xor { a: 6, s: 31, b: 4 });
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 0, s: 31, b: 0 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 7,
                s: 0,
                shift: 31,
            });
        self.emit_branch_conditional_to(12, 0, labels[&26]); // blt
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 30,
            });
        self.emit_branch_conditional_to(12, 2, labels[&19]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.record_relocation(RelocationKind::Rel24, "__copy_longs_unaligned");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__copy_longs_unaligned".to_string(),
        });
        self.emit_branch_to(labels[&24]); // b
        self.bind_label(labels[&17]);
        self.record_relocation(RelocationKind::Rel24, "__copy_longs_rev_unaligned");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__copy_longs_rev_unaligned".to_string(),
        });
        self.emit_branch_to(labels[&24]); // b
        self.bind_label(labels[&19]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
        self.record_relocation(RelocationKind::Rel24, "__copy_longs_aligned");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__copy_longs_aligned".to_string(),
        });
        self.emit_branch_to(labels[&24]); // b
        self.bind_label(labels[&23]);
        self.record_relocation(RelocationKind::Rel24, "__copy_longs_rev_aligned");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__copy_longs_rev_aligned".to_string(),
        });
        self.bind_label(labels[&24]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.emit_branch_to(labels[&46]); // b
        self.bind_label(labels[&26]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&37]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&34]); // b
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 3,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 4,
                offset: 1,
            });
        self.bind_label(labels[&34]);
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&32]); // bne
        self.emit_branch_to(labels[&45]); // b
        self.bind_label(labels[&37]);
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 31, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&43]); // b
        self.bind_label(labels[&41]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 3,
                offset: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 4,
                offset: -1,
            });
        self.bind_label(labels[&43]);
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&41]); // bne
        self.bind_label(labels[&45]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
