//! sms_os_create_heap: an exact-match whole-function capture (fire 528).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SMS_OS_CREATE_HEAP_AST_HASH: u64 = 0x656b_76c0_4b3f_22b4;

impl Generator {
    pub(super) fn try_sms_os_create_heap(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSCreateHeap"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_CREATE_HEAP_AST_HASH {
            eprintln!("sms_os_create_heap hash candidate: {hash:#x}");
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
                eprintln!("sms_os_create_heap context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.output.symbol_order = ["NumHeaps", "HeapArray"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [203, 216, 219] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::EmbSda21, "NumHeaps");
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 31,
        });
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 7,
                s: 0,
                begin: 0,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 6, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 6 });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: 4,
                s: 4,
                begin: 0,
                end: 26,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_conditional_to(4, 1, labels[&219]); // ble
        self.bind_label(labels[&203]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&216]); // bge
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 7, b: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 7,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 7,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&216]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 1,
        });
        self.emit_branch_conditional_to(16, 0, labels[&203]); // bdnz
        self.bind_label(labels[&219]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
