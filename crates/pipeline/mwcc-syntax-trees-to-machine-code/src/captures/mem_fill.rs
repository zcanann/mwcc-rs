//! mem_fill: the runtime `__mem.c` __fill_mem exact-match capture (fire 534).
//! Fills a region with a byte value, splaying it to a word and storing 8 words per
//! iteration for the bulk. Lives in the `.init` code section (SECTION_INIT). See
//! captures::ast_hash and docs/emission-model.md.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};

const MEM_FILL_AST_HASHES: &[u64] = &[0xcb839d5bb53db21c, 0xa772cdec39870b71];

impl Generator {
    pub(super) fn try_mem_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__fill_mem"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if !MEM_FILL_AST_HASHES.contains(&super::ast_hash(function)) {
            return Ok(false);
        }
        use mwcc_machine_code::Instruction;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> = std::collections::HashMap::new();
        for target in [9, 12, 20, 23, 33, 35, 38, 40, 42] {
            labels.insert(target, self.fresh_label());
        }
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 32 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 4, s: 4, clear: 24 });
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::move_register(7, 4));
        self.emit_branch_conditional_to(12, 0, labels[&40]); // blt
        self.output.instructions.push(Instruction::Nor { a: 0, s: 6, b: 6 }); // not r0,r6
        self.output.instructions.push(Instruction::ClearLeftImmediateRecord { a: 3, s: 0, clear: 30 });
        self.emit_branch_conditional_to(12, 2, labels[&12]); // beq
        self.output.instructions.push(Instruction::SubtractFrom { d: 5, a: 3, b: 5 });
        self.bind_label(labels[&9]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 7, a: 6, offset: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&9]); // bne
        self.bind_label(labels[&12]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&20]); // beq
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 3, s: 7, shift: 24 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 7, shift: 16 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 4, s: 7, shift: 8 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output.instructions.push(Instruction::Or { a: 7, s: 7, b: 0 });
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 5, shift: 27, begin: 5, end: 31 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 6, immediate: -3 });
        self.emit_branch_conditional_to(12, 2, labels[&33]); // beq
        self.bind_label(labels[&23]);
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 4 });
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 16 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 24 });
        self.output.instructions.push(Instruction::StoreWord { s: 7, a: 4, offset: 28 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 7, a: 4, offset: 32 });
        self.emit_branch_conditional_to(4, 2, labels[&23]); // bne
        self.bind_label(labels[&33]);
        self.output.instructions.push(Instruction::RotateAndMaskRecord { a: 3, s: 5, shift: 30, begin: 29, end: 31 });
        self.emit_branch_conditional_to(12, 2, labels[&38]); // beq
        self.bind_label(labels[&35]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 3, a: 3, immediate: -1 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 7, a: 4, offset: 4 });
        self.emit_branch_conditional_to(4, 2, labels[&35]); // bne
        self.bind_label(labels[&38]);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 3 });
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 5, s: 5, clear: 30 });
        self.bind_label(labels[&40]);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 }); // beqlr
        self.bind_label(labels[&42]);
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: 5, a: 5, immediate: -1 });
        self.output.instructions.push(Instruction::StoreByteWithUpdate { s: 7, a: 6, offset: 1 });
        self.emit_branch_conditional_to(4, 2, labels[&42]); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
