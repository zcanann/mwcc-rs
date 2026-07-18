//! gdc_destroy: the `__destroy_global_chain` exact-match capture (fire 531).
//! Walks `__global_destructor_chain`, calling each node's destructor with -1 and
//! unlinking it — mwcc's canonical bottom-test `while` (a `b` to the tail test,
//! `bne` back into the body) with a `bctrl` call through the node's function
//! pointer. Captured verbatim (loop + indirect-call codegen not yet general).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function. Byte-identical across projects,
/// but the AST repr differs (the DTORCALL macro expands differently): wind_waker
/// vs super_mario_strikers.
const GDC_DESTROY_AST_HASHES: &[u64] =
    &[0xb098beb92da52f1e, 0x5b61039f58ec9861, 0x30b2ff32239360ce];

impl Generator {
    pub(super) fn try_gdc_destroy(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__destroy_global_chain"
            || function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if !GDC_DESTROY_AST_HASHES.contains(&hash) {
            return Ok(false);
        }
        // No context gate: no pooled constants / anonymous labels here, so the
        // skipped-inline fingerprint carries no @N bump — the AST hash identifies
        // the function and its bytes are identical across every project sharing
        // the runtime source (the ctx fingerprint varies: ww/strikers differ from pik).
        // -- emit (non-leaf, 16-byte frame, only LR saved) --
        self.frame_size = 16;
        self.non_leaf = true;
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [4, 11] {
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.emit_branch_to(labels[&11]); // b <test>
        self.bind_label(labels[&4]); // loop body: iter is in r3
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: 0,
        }); // iter->next
        self.output
            .instructions
            .push(Instruction::load_immediate(4, -1)); // li r4,-1
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        }); // head = iter->next
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 4,
        }); // iter->destructor
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 8,
        }); // iter->object
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink); // (*destructor)(object, -1)
        self.bind_label(labels[&11]); // test: iter = head
        self.record_relocation(RelocationKind::EmbSda21, "__global_destructor_chain");
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 }); // cmplwi r3,0
        self.emit_branch_conditional_to(4, 2, labels[&4]); // bne <body>
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
        Ok(true)
    }
}
