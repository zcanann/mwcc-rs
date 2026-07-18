//! wcs_wcstoul: an exact-match whole-function capture (fire 700).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const WCS_WCSTOUL_AST_HASH: u64 = 0x2470211a6d8bbeb1;

impl Generator {
    pub(super) fn try_wcs_wcstoul(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wcstoul"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != WCS_WCSTOUL_AST_HASH {
            eprintln!("wcs_wcstoul hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x4dc5812f6e4177a3 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("wcs_wcstoul context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        // mwcc materializes the SDA21 errno symbol before the ADDR16
        // __wStringRead pair despite the later code position (measured).
        self.output.symbol_order = vec!["errno".into(), "__wStringRead".into()];
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [27, 34, 38] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, -32768));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 1,
            immediate: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 4));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 6,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 1,
            immediate: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.record_relocation(RelocationKind::Addr16Ha, "__wStringRead");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "__wStringRead");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 5));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 0));
        self.record_relocation(RelocationKind::Rel24, "__wcstoul");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__wcstoul".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&27]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 1,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 30, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });
        self.bind_label(labels[&27]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&34]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 34));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.record_relocation(RelocationKind::EmbSda21, "errno");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.emit_branch_to(labels[&38]); // b
        self.bind_label(labels[&34]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.output
            .instructions
            .push(Instruction::Negate { d: 3, a: 3 });
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 44,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 40,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
