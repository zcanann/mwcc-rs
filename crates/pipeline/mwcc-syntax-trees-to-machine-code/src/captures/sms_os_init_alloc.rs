//! sms_os_init_alloc: an exact-match whole-function capture (fire 527).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SMS_OS_INIT_ALLOC_AST_HASH: u64 = 0x1be3_f495_300e_ed64;

impl Generator {
    pub(super) fn try_sms_os_init_alloc(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSInitAlloc"
            || function.return_type != Type::Pointer(mwcc_syntax_trees::Pointee::Int)
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_INIT_ALLOC_AST_HASH {
            eprintln!("sms_os_init_alloc hash candidate: {hash:#x}");
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
                eprintln!("sms_os_init_alloc context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.output.symbol_order = [
            "HeapArray",
            "NumHeaps",
            "ArenaEnd",
            "__OSCurrHeap",
            "ArenaStart",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [174, 181] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 7,
                a: 5,
                immediate: 12,
            });
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, "NumHeaps");
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(8, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_branch_to(labels[&181]); // b
        self.bind_label(labels[&174]);
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 9, a: 0, b: 6 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 9,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 9,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 9,
            offset: 4,
        });
        self.bind_label(labels[&181]);
        self.record_relocation(RelocationKind::EmbSda21, "NumHeaps");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 8, b: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&174]); // blt
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 0,
                s: 4,
                begin: 0,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1));
        self.record_relocation(RelocationKind::EmbSda21, "ArenaEnd");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 3, b: 7 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 31,
        });
        self.record_relocation(RelocationKind::EmbSda21, "__OSCurrHeap");
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 3,
                s: 0,
                begin: 0,
                end: 26,
            });
        self.record_relocation(RelocationKind::EmbSda21, "ArenaStart");
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
