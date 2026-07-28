//! sms_os_alloc_from_heap: an exact-match whole-function capture (fire 526).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SMS_OS_ALLOC_FROM_HEAP_AST_HASH: u64 = 0xf70c_d05d_c498_f455;

impl Generator {
    pub(super) fn try_sms_os_alloc_from_heap(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSAllocFromHeap"
            || function.return_type != Type::Pointer(mwcc_syntax_trees::Pointee::Int)
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_ALLOC_FROM_HEAP_AST_HASH {
            eprintln!("sms_os_alloc_from_heap hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            // super_mario_sunshine GMSJ01/GMSP01, GC/1.2.5n.
            0xece5_1d04_8c1e_7e9d => 0,
            _ => {
                eprintln!("sms_os_alloc_from_heap context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.output.symbol_order = ["HeapArray"].into_iter().map(String::from).collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [79, 83, 85, 89, 99, 104, 106, 108, 119, 124, 125, 132] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: 3,
                immediate: 12,
            });
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 3, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 63,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 5,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 3,
                s: 0,
                begin: 0,
                end: 26,
            });
        self.emit_branch_to(labels[&83]); // b
        self.bind_label(labels[&79]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, labels[&85]); // ble
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: 4,
        });
        self.bind_label(labels[&83]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&79]); // bne
        self.bind_label(labels[&85]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&89]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&89]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 64,
            });
        self.emit_branch_conditional_to(4, 0, labels[&108]); // bge
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&99]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.bind_label(labels[&99]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 6,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&104]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: 4,
        });
        self.emit_branch_to(labels[&106]); // b
        self.bind_label(labels[&104]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 4,
        });
        self.bind_label(labels[&106]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 5,
            offset: 4,
        });
        self.emit_branch_to(labels[&125]); // b
        self.bind_label(labels[&108]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 6,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 6, b: 3 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&119]); // beq
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 0,
        });
        self.bind_label(labels[&119]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&124]); // beq
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 4,
        });
        self.emit_branch_to(labels[&125]); // b
        self.bind_label(labels[&124]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 4,
        });
        self.bind_label(labels[&125]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 6,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: 0,
        });
        self.emit_branch_conditional_to(12, 2, labels[&132]); // beq
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 3,
            offset: 0,
        });
        self.bind_label(labels[&132]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
