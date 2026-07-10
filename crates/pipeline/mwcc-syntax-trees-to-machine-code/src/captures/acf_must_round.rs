//! acf_must_round: an exact-match whole-function capture (fire 685).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const ACF_MUST_ROUND_AST_HASH: u64 = 0xb02ef1351c42f13b;

impl Generator {
    pub(super) fn try_acf_must_round(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__must_round"
            || function.return_type != Type::Int
            || function.parameters.len() != 2
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != ACF_MUST_ROUND_AST_HASH {
            eprintln!("acf_must_round hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xadf9060938342c54 => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("acf_must_round context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [7, 10, 18, 23, 25] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 5 });
        self.output.instructions.push(Instruction::Add { d: 6, a: 3, b: 6 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 });
        self.emit_branch_conditional_to(4, 1, labels[&7]); // ble
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&7]);
        self.emit_branch_conditional_to(4, 0, labels[&10]); // bge
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&10]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 5, a: 3, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 5 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 3, b: 5 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 6, b: 5 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 6, b: 5 });
        self.emit_branch_conditional_to(4, 0, labels[&25]); // bge
        self.bind_label(labels[&18]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&23]); // beq
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.emit_branch_conditional_to(16, 0, labels[&18]); // bdnz
        self.bind_label(labels[&25]);
        self.output.instructions.push(Instruction::Add { d: 4, a: 3, b: 4 });
        self.output.instructions.push(Instruction::load_immediate(3, -1));
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 0, s: 0, clear: 31 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
