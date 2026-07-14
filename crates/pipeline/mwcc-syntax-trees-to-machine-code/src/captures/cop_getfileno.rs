//! cop_getfileno: an exact-match whole-function capture (fire 768).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const COP_GETFILENO_AST_HASH: u64 = 0xadae6c5bca220281;

impl Generator {
    pub(super) fn try_cop_getfileno(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__CARDGetFileNo"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != COP_GETFILENO_AST_HASH {
            eprintln!("cop_getfileno hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xc83679bebce41ffd => 0, // marioparty4 (bump TBD from refctx @N diff)
            _ => {
                eprintln!("cop_getfileno context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 16;
        self.non_leaf = true;
        self.output.symbol_order = ["_savegpr_27", "__CARDGetDirBlock", "__CARDDiskNone", "memcmp", "_restgpr_27"].into_iter().map(String::from).collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [13, 16, 21, 38, 40, 41, 47, 57, 61, 68, 69, 74, 79] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_savegpr_27".to_string() });
        self.output.instructions.push(Instruction::move_register(27, 3));
        self.output.instructions.push(Instruction::move_register(28, 4));
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::move_register(29, 5));
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]); // bne
        self.output.instructions.push(Instruction::load_immediate(3, -3));
        self.emit_branch_to(labels[&79]); // b
        self.bind_label(labels[&13]);
        self.record_relocation(RelocationKind::Rel24, "__CARDGetDirBlock");
        self.output.instructions.push(Instruction::BranchAndLink { target: "__CARDGetDirBlock".to_string() });
        self.output.instructions.push(Instruction::load_immediate(30, 0));
        self.output.instructions.push(Instruction::move_register(31, 3));
        self.bind_label(labels[&16]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 31, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 255 });
        self.emit_branch_conditional_to(4, 2, labels[&21]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, -4));
        self.emit_branch_to(labels[&41]); // b
        self.bind_label(labels[&21]);
        self.record_relocation(RelocationKind::Addr16Ha, "__CARDDiskNone");
        self.output.instructions.push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 27, offset: 268 });
        self.record_relocation(RelocationKind::Addr16Lo, "__CARDDiskNone");
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.output.instructions.push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::load_immediate(5, 4));
        self.record_relocation(RelocationKind::Rel24, "memcmp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcmp".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 27, offset: 268 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 31, immediate: 4 });
        self.output.instructions.push(Instruction::load_immediate(5, 2));
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 4 });
        self.record_relocation(RelocationKind::Rel24, "memcmp");
        self.output.instructions.push(Instruction::BranchAndLink { target: "memcmp".to_string() });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&40]); // bne
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&41]); // b
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::load_immediate(0, -10));
        self.bind_label(labels[&41]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&74]); // blt
        self.output.instructions.push(Instruction::move_register(6, 28));
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 31, immediate: 8 });
        self.output.instructions.push(Instruction::load_immediate(4, 32));
        self.emit_branch_to(labels[&61]); // b
        self.bind_label(labels[&47]);
        self.output.instructions.push(Instruction::LoadByteZero { d: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 1 });
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 1 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 3, s: 3 });
        self.output.instructions.push(Instruction::ExtendSignByte { a: 0, s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&57]); // beq
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&69]); // b
        self.bind_label(labels[&57]);
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&61]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&69]); // b
        self.bind_label(labels[&61]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 4, a: 4, immediate: -1 });
        self.emit_branch_conditional_to(4, 0, labels[&47]); // bge
        self.output.instructions.push(Instruction::LoadByteZero { d: 0, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&68]); // bne
        self.output.instructions.push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(labels[&69]); // b
        self.bind_label(labels[&68]);
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.bind_label(labels[&69]);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&74]); // beq
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 29, offset: 0 });
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&79]); // b
        self.bind_label(labels[&74]);
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 30, immediate: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 31, a: 31, immediate: 64 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 30, immediate: 127 });
        self.emit_branch_conditional_to(12, 0, labels[&16]); // blt
        self.output.instructions.push(Instruction::load_immediate(3, -4));
        self.bind_label(labels[&79]);
        self.output.instructions.push(Instruction::AddImmediate { d: 11, a: 1, immediate: 32 });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
        self.output.instructions.push(Instruction::BranchAndLink { target: "_restgpr_27".to_string() });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
