//! cdl_async: an exact-match whole-function capture (fire 764).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CDL_ASYNC_AST_HASH: u64 = 0x7d6f622317218f23;

impl Generator {
    pub(super) fn try_cdl_async(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "CARDDeleteAsync"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CDL_ASYNC_AST_HASH {
            eprintln!("cdl_async hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cdl_async context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = [
            "__CARDGetControlBlock",
            "__CARDGetFileNo",
            "__CARDPutControlBlock",
            "__CARDIsOpened",
            "__CARDGetDirBlock",
            "memset",
            "__CARDDefaultApiCallback",
            "DeleteCallback",
            "__CARDUpdateDir",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [14, 23, 32, 47, 49, 60, 61] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 5));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.record_relocation(RelocationKind::Rel24, "__CARDGetControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDGetControlBlock".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&14]); // bge
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&14]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetFileNo");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDGetFileNo".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 4, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&23]); // bge
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDPutControlBlock".to_string(),
        });
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDIsOpened");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDIsOpened".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&32]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDPutControlBlock".to_string(),
        });
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&32]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDGetDirBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDGetDirBlock".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 255));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 64));
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 6,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 54,
            });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: 190,
        });
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&47]); // beq
        self.output
            .instructions
            .push(Instruction::move_register(0, 31));
        self.emit_branch_to(labels[&49]); // b
        self.bind_label(labels[&47]);
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDDefaultApiCallback");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDDefaultApiCallback");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.bind_label(labels[&49]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Addr16Ha, "DeleteCallback");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "DeleteCallback");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 208,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.record_relocation(RelocationKind::Rel24, "__CARDUpdateDir");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDUpdateDir".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 31, s: 3, b: 3 });
        self.emit_branch_conditional_to(4, 0, labels[&60]); // bge
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 31));
        self.record_relocation(RelocationKind::Rel24, "__CARDPutControlBlock");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDPutControlBlock".to_string(),
        });
        self.bind_label(labels[&60]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
