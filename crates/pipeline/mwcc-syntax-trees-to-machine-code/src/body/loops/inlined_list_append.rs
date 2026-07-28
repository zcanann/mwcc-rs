//! Compose a verified list append helper into a list-construction caller.
//!
//! Deferred automatic inlining keeps the helper's address-taken allocation
//! slot in the caller frame, carries the rounded item size across the embedded
//! append, and addresses the file-local registry through the BSS section
//! anchor. Those choices form one interprocedural scheduling transaction.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::DataSectionDisplacement;

use super::linked_list_append::{classify as classify_append, LinkedListAppend};
mod recognize;
use recognize::{classify, InlinedListConstruction};

impl Generator {
    pub(crate) fn try_inlined_list_append(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let Some(helper) = self
            .inline_bodies
            .definition_body(&shape.helper)
            .and_then(classify_append)
        else {
            return Ok(false);
        };
        if !self.skipped_inline_names.contains(&shape.padding_helper)
            || !self.full_bss_globals.contains(&shape.registry)
            || !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
            || helper.item_size_offset != shape.size_offset
            || helper.item_count_offset != shape.count_offset
            || helper.head_offset != shape.head_offset
        {
            return Ok(false);
        }
        self.emit_inlined_list_append(&shape, &helper);
        Ok(true)
    }

    fn emit_inlined_list_append(
        &mut self,
        shape: &InlinedListConstruction,
        helper: &LinkedListAppend,
    ) {
        const BSS_ANCHOR: &str = "...bss.0";
        const LIST_OUT: u8 = 31;
        const REGISTRY: u8 = 30;
        const ITEM_SIZE: u8 = 29;
        const NODE_SLOT: i16 = 20;
        let allocation_succeeded = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();
        let loop_advance = self.fresh_label();
        let append_result = self.fresh_label();
        let construction_failed = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![LIST_OUT, REGISTRY, ITEM_SIZE];
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(5, BSS_ANCHOR);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: shape.alignment_bias,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: LIST_OUT,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: LIST_OUT,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: NODE_SLOT,
            },
            Instruction::StoreWord {
                s: REGISTRY,
                a: 1,
                offset: 32,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, BSS_ANCHOR);
        self.output
            .data_section_displacements
            .push(DataSectionDisplacement {
                instruction_index: self.output.instructions.len(),
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    shape.registry.clone(),
                ),
            });
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: REGISTRY,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: ITEM_SIZE,
                a: 1,
                offset: 28,
            },
            Instruction::RotateAndMask {
                a: ITEM_SIZE,
                s: 0,
                shift: 0,
                begin: 0,
                end: 31 - shape.alignment_bits,
            },
            Instruction::LoadWord {
                d: 5,
                a: REGISTRY,
                offset: helper.item_size_offset,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 5,
                immediate: helper.node_header_size,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &helper.allocator);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: helper.allocator.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, allocation_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.emit_branch_to(append_result);

        self.bind_label(allocation_succeeded);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::load_immediate(0, 0),
            Instruction::AddImmediate {
                d: 5,
                a: REGISTRY,
                immediate: helper.head_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: helper.node_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: LIST_OUT,
                offset: 0,
            },
        ]);
        self.emit_branch_to(loop_condition);

        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 5,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, loop_advance);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::load_immediate(4, 1),
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: REGISTRY,
                offset: helper.item_count_offset,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 0,
                a: REGISTRY,
                offset: helper.item_count_offset,
            },
        ]);
        self.emit_branch_to(append_result);

        self.bind_label(loop_advance);
        self.output
            .instructions
            .push(Instruction::move_register(5, 0));
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));

        self.bind_label(append_result);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, construction_failed);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: LIST_OUT,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::load_immediate(3, 1),
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.count_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: LIST_OUT,
                offset: 0,
            },
            Instruction::StoreWord {
                s: ITEM_SIZE,
                a: 4,
                offset: shape.size_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: LIST_OUT,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.next_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: LIST_OUT,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.head_offset,
            },
        ]);
        self.emit_branch_to(epilogue);
        self.bind_label(construction_failed);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::LoadWord {
                d: LIST_OUT,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: REGISTRY,
                a: 1,
                offset: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord {
                d: ITEM_SIZE,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 40,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
