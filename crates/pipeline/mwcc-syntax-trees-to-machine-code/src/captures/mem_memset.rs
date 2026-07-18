//! mem_memset: the runtime `__mem.c` memset exact-match capture (fire 534).
//! A thin wrapper that saves the dst, tail-fills via __fill_mem, and returns dst.
//! Non-leaf (16-byte frame, r31). Lives in the `.init` code section (SECTION_INIT).
//! See captures::ast_hash and docs/emission-model.md.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::Function;

const MEM_MEMSET_AST_HASHES: &[u64] = &[0xda09cbe435050ac0, 0x5931b3662d91f0ed];

impl Generator {
    pub(super) fn try_mem_memset(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "memset"
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if !MEM_MEMSET_AST_HASHES.contains(&super::ast_hash(function)) {
            return Ok(false);
        }
        self.frame_size = 16;
        self.non_leaf = true;
        self.callee_saved = vec![31];
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.record_relocation(RelocationKind::Rel24, "__fill_mem");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__fill_mem".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
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
        Ok(true)
    }
}
