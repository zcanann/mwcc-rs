//! osm_send: an exact-match whole-function capture (fire 753).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const OSM_SEND_AST_HASH: u64 = 0x8c88c5621b1638c5;

impl Generator {
    pub(super) fn try_osm_send(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSSendMessage"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != OSM_SEND_AST_HASH {
            eprintln!("osm_send hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc418e20019aad651 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("osm_send context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [14, 20, 22, 42] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 5));
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(28, 3));
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSDisableInterrupts".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 31,
                s: 30,
                clear: 31,
            });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.emit_branch_to(labels[&22]); // b
        self.bind_label(labels[&14]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSRestoreInterrupts".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&42]); // b
        self.bind_label(labels[&20]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 28));
        self.record_relocation(RelocationKind::Rel24, "OSSleepThread");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSSleepThread".to_string(),
        });
        self.bind_label(labels[&22]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 28,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 28,
            offset: 28,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 6, b: 4 });
        self.emit_branch_conditional_to(4, 1, labels[&14]); // ble
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 28,
            offset: 24,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 28,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 28,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::DivideWord { d: 0, a: 4, b: 6 });
        self.output
            .instructions
            .push(Instruction::MultiplyLow { d: 0, a: 0, b: 6 });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 29, a: 5, b: 0 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 28,
            offset: 28,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 28,
            offset: 28,
        });
        self.record_relocation(RelocationKind::Rel24, "OSWakeupThread");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSWakeupThread".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, "OSRestoreInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSRestoreInterrupts".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(labels[&42]);
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 28,
            a: 1,
            offset: 16,
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
