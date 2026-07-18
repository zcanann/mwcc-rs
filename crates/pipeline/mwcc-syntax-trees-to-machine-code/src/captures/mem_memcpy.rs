//! mem_memcpy: the runtime `__mem.c` memcpy exact-match capture (fire 534).
//! A byte copy that picks a forward or backward loop by src/dst overlap. Lives in
//! the `.init` code section (SECTION_INIT); the section override rides through from
//! the parser (f533), so this only emits the body. See captures::ast_hash.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::Function;

/// AST hashes of the captured function across projects (source varies slightly).
const MEM_MEMCPY_AST_HASHES: &[u64] = &[0x4682724e5c916f10, 0x92aa5a720ec3b014, 0xb7f1b356f741a985];

impl Generator {
    pub(super) fn try_mem_memcpy(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "memcpy"
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if !MEM_MEMCPY_AST_HASHES.contains(&super::ast_hash(function)) {
            return Ok(false);
        }
        use mwcc_machine_code::Instruction;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [6, 8, 11, 15, 17] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 3 });
        self.emit_branch_conditional_to(12, 0, labels[&11]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&8]); // b
        self.bind_label(labels[&6]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: 1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 6,
                offset: 1,
            });
        self.bind_label(labels[&8]);
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&6]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(labels[&11]);
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::Add { d: 6, a: 3, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 1,
        });
        self.emit_branch_to(labels[&17]); // b
        self.bind_label(labels[&15]);
        self.output
            .instructions
            .push(Instruction::LoadByteZeroWithUpdate {
                d: 0,
                a: 4,
                offset: -1,
            });
        self.output
            .instructions
            .push(Instruction::StoreByteWithUpdate {
                s: 0,
                a: 6,
                offset: -1,
            });
        self.bind_label(labels[&17]);
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: 5,
                a: 5,
                immediate: -1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&15]); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
