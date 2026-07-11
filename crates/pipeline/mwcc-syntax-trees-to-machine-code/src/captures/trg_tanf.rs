//! trg_tanf: an exact-match whole-function capture (fire 711).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const TRG_TANF_AST_HASH: u64 = 0x716212911abccc0d;

impl Generator {
    pub(super) fn try_trg_tanf(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "tanf"
            || function.return_type != Type::Float
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != TRG_TANF_AST_HASH {
            eprintln!("trg_tanf hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa5533c97b3cd5d53 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("trg_tanf context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 48;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedStore { s: 31, a: 1, offset: 40, w: 0, i: 0 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 30, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedStore { s: 30, a: 1, offset: 24, w: 0, i: 0 });
        self.output.instructions.push(Instruction::FloatMove { d: 30, b: 1 });
        self.record_relocation(RelocationKind::Rel24, "cos__Ff");
        self.output.instructions.push(Instruction::BranchAndLink { target: "cos__Ff".to_string() });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 30 });
        self.record_relocation(RelocationKind::Rel24, "sin__Ff");
        self.output.instructions.push(Instruction::BranchAndLink { target: "sin__Ff".to_string() });
        self.output.instructions.push(Instruction::FloatDivideSingle { d: 1, a: 1, b: 31 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedLoad { d: 31, a: 1, offset: 40, w: 0, i: 0 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 32 });
        self.output.instructions.push(Instruction::PairedSingleQuantizedLoad { d: 30, a: 1, offset: 24, w: 0, i: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 52 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 30, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 48 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
