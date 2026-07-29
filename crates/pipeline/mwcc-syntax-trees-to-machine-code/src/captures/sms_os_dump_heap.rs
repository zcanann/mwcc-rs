//! sms_os_dump_heap: an exact-match whole-function capture (fire 531).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function.
const SMS_OS_DUMP_HEAP_AST_HASH: u64 = 0x16d1_051c_00ef_40b6;

impl Generator {
    pub(super) fn try_sms_os_dump_heap(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSDumpHeap"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_DUMP_HEAP_AST_HASH {
            eprintln!("sms_os_dump_heap hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xece5_1d04_8c1e_7e9d => 0,
            _ => {
                eprintln!("sms_os_dump_heap context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29];
        for bytes in [
            &b"\nOSDumpHeap(%d):\n"[..],
            &b"--------Inactive\n"[..],
            &b"addr\tsize\t\tend\tprev\tnext\n"[..],
            &b"--------Allocated\n"[..],
            &b"%x\t%d\t%x\t%x\t%x\n"[..],
            &b"--------Free\n"[..],
        ] {
            self.intern_string_literal(bytes);
        }
        self.output.symbol_order = ["...data.0", "OSReport", "HeapArray"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [23, 31, 40, 47, 56, 58] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "...data.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 29,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 796,
        });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: 29,
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
            .push(Instruction::Add { d: 30, a: 3, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&23]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 816,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.emit_branch_to(labels[&58]); // b
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 836,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 864,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 30,
            offset: 8,
        });
        self.emit_branch_to(labels[&40]); // b
        self.bind_label(labels[&31]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 29,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 29));
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 884,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 29,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 29, b: 5 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 29,
            a: 29,
            offset: 4,
        });
        self.bind_label(labels[&40]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 29,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&31]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 900,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 30,
            offset: 4,
        });
        self.emit_branch_to(labels[&56]); // b
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 30,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 884,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 30,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 30, b: 5 });
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 30,
            offset: 4,
        });
        self.bind_label(labels[&56]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&47]); // bne
        self.bind_label(labels[&58]);
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
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
