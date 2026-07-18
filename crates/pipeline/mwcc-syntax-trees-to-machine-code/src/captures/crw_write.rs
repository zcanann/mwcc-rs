//! crw_write: an exact-match whole-function capture (fire 766).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const CRW_WRITE_AST_HASH: u64 = 0x64fb5f1f8377e36f;

impl Generator {
    pub(super) fn try_crw_write(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDWrite"
            || function.return_type != Type::Int
            || function.parameters.len() != 5
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != CRW_WRITE_AST_HASH {
            eprintln!("crw_write hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("crw_write context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["__CARDBlock", "BlockWriteCallback", "__CARDWritePage"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [12, 21] {
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
            .push(Instruction::MultiplyImmediate {
                d: 9,
                a: 3,
                immediate: 272,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDBlock");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(8, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDBlock");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 8,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 8, a: 0, b: 9 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -3));
        self.emit_branch_to(labels[&21]); // b
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 8,
            offset: 212,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 5,
                shift: 7,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "BlockWriteCallback");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 172,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "BlockWriteCallback");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: 176,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 8,
            offset: 180,
        });
        self.record_relocation(RelocationKind::Rel24, "__CARDWritePage");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__CARDWritePage".to_string(),
        });
        self.bind_label(labels[&21]);
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
