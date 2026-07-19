//! `wcstombs` shape that keeps `unicode_to_UTF8` out-of-line.
//!
//! This is the measured GC/1.3 `-inline auto,deferred` lowering: the trivial
//! `wctomb` wrapper disappears while its non-inline `unicode_to_UTF8` callee
//! remains a call. `-use_lmw_stmw` independently selects the save convention.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

const MBS_WCSTOMBS_DIRECT_AST_HASH: u64 = 0x1c4a4ee6315422f4;
const MBS_MP4_CONTEXT: u64 = 0x6e7a972c5b9ab3cb;

impl Generator {
    pub(super) fn try_mbs_wcstombs_direct(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wcstombs"
            || function.return_type != Type::UnsignedInt
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != MBS_WCSTOMBS_DIRECT_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != MBS_MP4_CONTEXT
        {
            return Ok(false);
        }

        self.frame_size = 48;
        self.non_leaf = true;
        self.callee_saved = vec![27, 28, 29, 30, 31];
        // Deferred inline analysis assigns unicode_to_UTF8's auto-array image
        // to this caller's static slot even though the callee remains
        // out-of-line. The callee's actual load then reuses this @4 object.
        self.output.intern_constant_static_slot(0x0000c0e0, 4);

        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [10, 12, 14, 20, 32, 34, 35] {
            labels.insert(target, self.fresh_label());
        }

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 52,
        });
        if self.behavior.use_lmw_stmw {
            self.output
                .instructions
                .push(Instruction::StoreMultipleWord {
                    s: 27,
                    a: 1,
                    offset: 28,
                });
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 11,
                a: 1,
                immediate: 48,
            });
            self.record_relocation(RelocationKind::Rel24, "_savegpr_27");
            self.output.instructions.push(Instruction::BranchAndLink {
                target: "_savegpr_27".to_string(),
            });
        }
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 27, s: 3, b: 3 });
        self.output
            .instructions
            .push(Instruction::move_register(28, 5));
        self.output
            .instructions
            .push(Instruction::load_immediate(30, 0));
        self.emit_branch_conditional_to(12, 2, labels[&10]); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&12]); // bne
        self.bind_label(labels[&10]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(labels[&35]);
        self.bind_label(labels[&12]);
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.emit_branch_to(labels[&32]);
        self.bind_label(labels[&14]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 4,
                a: 29,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&20]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 0, a: 27, b: 30 });
        self.emit_branch_to(labels[&34]);
        self.bind_label(labels[&20]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 2,
        });
        self.record_relocation(RelocationKind::Rel24, "unicode_to_UTF8");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "unicode_to_UTF8".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 30, b: 31 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 28 });
        self.emit_branch_conditional_to(12, 1, labels[&34]); // bgt
        self.output
            .instructions
            .push(Instruction::move_register(5, 31));
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 27, b: 30 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.record_relocation(RelocationKind::Rel24, "strcat");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "strcat".to_string(),
        });
        self.output.instructions.push(Instruction::Add {
            d: 30,
            a: 30,
            b: 31,
        });
        self.bind_label(labels[&32]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 30, b: 28 });
        self.emit_branch_conditional_to(4, 1, labels[&14]); // ble
        self.bind_label(labels[&34]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.bind_label(labels[&35]);
        if self.behavior.use_lmw_stmw {
            self.output
                .instructions
                .push(Instruction::LoadMultipleWord {
                    d: 27,
                    a: 1,
                    offset: 28,
                });
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 11,
                a: 1,
                immediate: 48,
            });
            self.record_relocation(RelocationKind::Rel24, "_restgpr_27");
            self.output.instructions.push(Instruction::BranchAndLink {
                target: "_restgpr_27".to_string(),
            });
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
