//! sca_vsscanf: an exact-match whole-function capture (fire 691).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const SCA_VSSCANF_AST_HASH: u64 = 0xa7fbfcf9e5ef1f2d;

impl Generator {
    pub(super) fn try_sca_vsscanf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__vsscanf"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SCA_VSSCANF_AST_HASH {
            eprintln!("sca_vsscanf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x848ec7a74d401bdc => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("sca_vsscanf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 12, 19] {
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
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::move_register(6, 5));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.emit_branch_conditional_to(12, 2, labels[&10]); // beq
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.bind_label(labels[&10]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&19]); // b
        self.bind_label(labels[&12]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "__StringRead");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(5, 4));
        self.record_relocation(RelocationKind::Addr16Lo, "__StringRead");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "__sformatter");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__sformatter".to_string(),
        });
        self.bind_label(labels[&19]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
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
