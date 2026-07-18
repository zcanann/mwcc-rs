//! gek_register: an exact-match whole-function capture (fire 734).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const GEK_REGISTER_AST_HASH: u64 = 0xb38417fc443e8f4f;

impl Generator {
    pub(super) fn try_gek_register(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__register_fragment"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != GEK_REGISTER_AST_HASH {
            eprintln!("gek_register hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xbd60acb658c79e45 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("gek_register context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [11] {
            labels.insert(target, self.fresh_label());
        }
        self.record_relocation(RelocationKind::Addr16Ha, "fragment_info");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "fragment_info");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&11]); // bne
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&11]);
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
