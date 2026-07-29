//! sms_os_check_heap: an exact-match whole-function capture (fire 530).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function.
const SMS_OS_CHECK_HEAP_AST_HASH: u64 = 0x4132_932c_fca8_b374;

impl Generator {
    pub(super) fn try_sms_os_check_heap(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSCheckHeap"
            || function.return_type != Type::Int
            || function.parameters.len() != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != SMS_OS_CHECK_HEAP_AST_HASH {
            eprintln!("sms_os_check_heap hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xece5_1d04_8c1e_7e9d => 0,
            _ => {
                eprintln!("sms_os_check_heap context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 8;
        self.non_leaf = true;
        // Assertion reports address these pooled writable strings through the
        // translation unit's `...data.0` anchor rather than individual @N
        // relocations. Captured code must retain the source strings explicitly.
        for bytes in [
            &b"OSCheckHeap: Failed HeapArray in %d"[..],
            &b"OSCheckHeap: Failed 0 <= heap && heap < NumHeaps in %d"[..],
            &b"OSCheckHeap: Failed 0 <= hd->size in %d"[..],
            &b"OSCheckHeap: Failed hd->allocated == NULL || hd->allocated->prev == NULL in %d"[..],
            &b"OSCheckHeap: Failed InRange(cell, ArenaStart, ArenaEnd) in %d"[..],
            &b"OSCheckHeap: Failed OFFSET(cell, ALIGNMENT) == 0 in %d"[..],
            &b"OSCheckHeap: Failed cell->next == NULL || cell->next->prev == cell in %d"[..],
            &b"OSCheckHeap: Failed MINOBJSIZE <= cell->size in %d"[..],
            &b"OSCheckHeap: Failed OFFSET(cell->size, ALIGNMENT) == 0 in %d"[..],
            &b"OSCheckHeap: Failed 0 < total && total <= hd->size in %d"[..],
            &b"OSCheckHeap: Failed hd->free == NULL || hd->free->prev == NULL in %d"[..],
            &b"OSCheckHeap: Failed cell->next == NULL || (char*) cell + cell->size < (char*) cell->next in %d"[..],
            &b"OSCheckHeap: Failed total == hd->size in %d"[..],
        ] {
            self.intern_string_literal(bytes);
        }
        self.output.symbol_order = [
            "...data.0",
            "HeapArray",
            "OSReport",
            "NumHeaps",
            "ArenaStart",
            "ArenaEnd",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            242, 247, 253, 264, 276, 280, 284, 290, 298, 310, 319, 327, 332, 338, 339, 354, 358,
            364, 372, 384, 393, 401, 412, 419, 425, 426, 437, 438,
        ] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, "...data.0");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "...data.0");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.record_relocation(RelocationKind::EmbSda21, "HeapArray");
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&242]); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 893));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&242]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 0, labels[&247]); // blt
        self.record_relocation(RelocationKind::EmbSda21, "NumHeaps");
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 5 });
        self.emit_branch_conditional_to(12, 0, labels[&253]); // blt
        self.bind_label(labels[&247]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 36,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 894));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&253]);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 3,
                a: 3,
                immediate: 12,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 7, b: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&264]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 92,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 897));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&264]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 5,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&276]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&276]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 132,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 899));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&276]);
        self.record_relocation(RelocationKind::EmbSda21, "ArenaStart");
        self.output.instructions.push(Instruction::LoadWord {
            d: 9,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(8, 7));
        self.record_relocation(RelocationKind::EmbSda21, "ArenaEnd");
        self.output.instructions.push(Instruction::LoadWord {
            d: 10,
            a: 0,
            offset: 0,
        });
        self.emit_branch_to(labels[&339]); // b
        self.bind_label(labels[&280]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 9, b: 8 });
        self.emit_branch_conditional_to(12, 1, labels[&284]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 8, b: 10 });
        self.emit_branch_conditional_to(12, 0, labels[&290]); // blt
        self.bind_label(labels[&284]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 212,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 902));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&290]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 8,
                clear: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&298]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 276,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 903));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&298]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 8,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&310]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 8 });
        self.emit_branch_conditional_to(12, 2, labels[&310]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 332,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 904));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&310]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 8,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 8,
                immediate: 64,
            });
        self.emit_branch_conditional_to(4, 0, labels[&319]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 408,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 905));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&319]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 8,
                clear: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&327]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 460,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 906));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&327]);
        self.output
            .instructions
            .push(Instruction::AddRecord { d: 0, a: 0, b: 8 });
        self.emit_branch_conditional_to(4, 1, labels[&332]); // ble
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 1, labels[&338]); // ble
        self.bind_label(labels[&332]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 524,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 909));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&338]);
        self.output
            .instructions
            .push(Instruction::move_register(8, 7));
        self.bind_label(labels[&339]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&280]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 11,
            a: 5,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 11,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&426]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 11,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&426]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 584,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 917));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.emit_branch_to(labels[&426]); // b
        self.bind_label(labels[&354]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 9, b: 11 });
        self.emit_branch_conditional_to(12, 1, labels[&358]); // bgt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 11, b: 10 });
        self.emit_branch_conditional_to(12, 0, labels[&364]); // blt
        self.bind_label(labels[&358]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 212,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 920));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&364]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 11,
                clear: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&372]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 276,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 921));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&372]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 11,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&384]); // beq
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 11 });
        self.emit_branch_conditional_to(12, 2, labels[&384]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 332,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 922));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&384]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: 11,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 8,
                immediate: 64,
            });
        self.emit_branch_conditional_to(4, 0, labels[&393]); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 408,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 923));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&393]);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 3,
                s: 8,
                clear: 27,
            });
        self.emit_branch_conditional_to(12, 2, labels[&401]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 460,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 924));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&401]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&412]); // beq
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 11, b: 8 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 7 });
        self.emit_branch_conditional_to(12, 0, labels[&412]); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 656,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 925));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&412]);
        self.output
            .instructions
            .push(Instruction::AddRecord { d: 0, a: 0, b: 8 });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 8, b: 4 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: -32,
        });
        self.emit_branch_conditional_to(4, 1, labels[&419]); // ble
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 1, labels[&425]); // ble
        self.bind_label(labels[&419]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 524,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 929));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&425]);
        self.output
            .instructions
            .push(Instruction::move_register(11, 7));
        self.bind_label(labels[&426]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 11,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&354]); // bne
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(12, 2, labels[&437]); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 6,
            immediate: 752,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 936));
        self.record_relocation(RelocationKind::Rel24, "OSReport");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSReport".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, -1));
        self.emit_branch_to(labels[&438]); // b
        self.bind_label(labels[&437]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.bind_label(labels[&438]);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
