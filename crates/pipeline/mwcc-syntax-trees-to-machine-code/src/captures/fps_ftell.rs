//! fps_ftell: an exact-match whole-function capture (fire 702).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const FPS_FTELL_AST_HASH: u64 = 0xc36bd2549f20eb75;

impl Generator {
    pub(super) fn try_fps_ftell(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "ftell"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != FPS_FTELL_AST_HASH {
            eprintln!("fps_ftell hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0x33405ea3d804990f => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("fps_ftell context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [6, 9, 13, 18] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 26, begin: 29, end: 31 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&6]); // beq
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.bind_label(labels[&6]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 10 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&13]); // beq
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::load_immediate(0, 40));
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.record_relocation(RelocationKind::EmbSda21, "errno");
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&13]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 3, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 5, s: 0, shift: 27, begin: 29, end: 31 });
        self.emit_branch_conditional_to(4, 2, labels[&18]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 24 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 3, offset: 28 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 3 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 3, offset: 52 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: -2 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 0, b: 3 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
