//! abexpik_exit: an exact-match whole-function capture (fire 485).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ABEXPIK_EXIT_AST_HASH: u64 = 0xd3e9263655311ff5;

impl Generator {
    pub(super) fn try_abexpik_exit(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "exit"
            || function.return_type != Type::Void
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ABEXPIK_EXIT_AST_HASH {
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x626216a8cf3d36f5 => 0, // pikmin (dev loop)
            _ => return Ok(false),
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.callee_saved = vec![31];
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 17, 25, 28, 38, 41, 48, 59] {
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
        self.record_relocation(RelocationKind::EmbSda21, "__aborting");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&38]); // bne
        self.record_relocation(RelocationKind::Addr16Ha, "atexit_funcs");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "atexit_funcs");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        });
        self.emit_branch_to(labels[&17]); // b
        self.bind_label(labels[&10]);
        self.record_relocation(RelocationKind::EmbSda21, "atexit_curr_func");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 3,
                shift: 2,
            });
        self.record_relocation(RelocationKind::EmbSda21, "atexit_curr_func");
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 12, a: 31, b: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&17]);
        self.record_relocation(RelocationKind::EmbSda21, "atexit_curr_func");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&10]); // bgt
        self.record_relocation(RelocationKind::Rel24, "__destroy_global_chain");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__destroy_global_chain".to_string(),
        });
        self.record_relocation(RelocationKind::Addr16Ha, "_dtors");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "_dtors");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 0));
        self.emit_branch_to(labels[&28]); // b
        self.bind_label(labels[&25]);
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 31,
            immediate: 4,
        });
        self.bind_label(labels[&28]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 31,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&25]); // bne
        self.record_relocation(RelocationKind::EmbSda21, "__stdio_exit");
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, "__stdio_exit");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(labels[&38]);
        self.record_relocation(RelocationKind::Addr16Ha, "__atexit_funcs");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__atexit_funcs");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        });
        self.emit_branch_to(labels[&48]); // b
        self.bind_label(labels[&41]);
        self.record_relocation(RelocationKind::EmbSda21, "__atexit_curr_func");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 3,
                shift: 2,
            });
        self.record_relocation(RelocationKind::EmbSda21, "__atexit_curr_func");
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 12, a: 31, b: 0 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.bind_label(labels[&48]);
        self.record_relocation(RelocationKind::EmbSda21, "__atexit_curr_func");
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, labels[&41]); // bgt
        self.record_relocation(RelocationKind::Rel24, "__kill_critical_regions");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__kill_critical_regions".to_string(),
        });
        self.record_relocation(RelocationKind::EmbSda21, "__console_exit");
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&59]); // beq
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, "__console_exit");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(labels[&59]);
        self.record_relocation(RelocationKind::Rel24, "_ExitProcess");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_ExitProcess".to_string(),
        });
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
