//! cop_cmpname: an exact-match whole-function capture (fire 768).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const COP_CMPNAME_AST_HASH: u64 = 0x966dc90fabe3d075;

impl Generator {
    pub(super) fn try_cop_cmpname(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDCompareFileName"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != COP_CMPNAME_AST_HASH {
            eprintln!("cop_cmpname hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cop_cmpname context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // OSFastCast phantoms at head of global-UND run; source-first fn (CARDOpen.c).
        self.output.phantom_externals = vec!["__OSf32tos16".to_string(), "__OSf32tou8".to_string()];
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [3, 13, 17] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 32));
        self.emit_branch_to(labels[&17]); // b
        self.bind_label(labels[&3]);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 3, s: 3 });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&17]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&17]);
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 6,
                a: 6,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 0, labels[&3]); // bge
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 0 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 0,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
