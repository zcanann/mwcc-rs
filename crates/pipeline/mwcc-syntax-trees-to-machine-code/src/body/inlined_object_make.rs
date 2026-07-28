//! Compose payload lookup and construction helpers into object creation.
//!
//! A caller-local payload slot is shared by the inlined list walk, registry
//! insertion, nested list construction, object allocation, and two event
//! callbacks. The complete transaction owns that frame and register schedule.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, InlinedObjectMake};

impl Generator {
    pub(crate) fn try_inlined_object_make(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function, &self.inline_bodies) else {
            return Ok(false);
        };
        if !self.globals.contains_key(&shape.registry)
            || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_inlined_object_make(&shape);
        Ok(true)
    }

    fn emit_inlined_object_make(&mut self, shape: &InlinedObjectMake) {
        const OUTPUT: u8 = 28;
        const ARGUMENT: u8 = 29;
        const OBJECT_TYPE: u8 = 30;
        const CREATED: u8 = 31;
        const PAYLOAD_SLOT: i16 = 20;
        let find_body = self.fresh_label();
        let find_condition = self.fresh_label();
        let find_advance = self.fresh_label();
        let find_done = self.fresh_label();
        let make_item_succeeded = self.fresh_label();
        let make_list_succeeded = self.fresh_label();
        let make_done = self.fresh_label();
        let payload_created = self.fresh_label();
        let payload_found = self.fresh_label();
        let object_item_succeeded = self.fresh_label();
        let final_callback = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![CREATED, OBJECT_TYPE, ARGUMENT, OUTPUT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: CREATED,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: OBJECT_TYPE,
                a: 1,
                offset: 32,
            },
            Instruction::move_register(OBJECT_TYPE, 5),
            Instruction::StoreWord {
                s: ARGUMENT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: ARGUMENT,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: OUTPUT,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: OUTPUT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, &shape.registry);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 6,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 6,
                offset: shape.head_offset,
            },
        ]);
        self.emit_branch_to(find_condition);
        self.bind_label(find_body);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: shape.node_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.type_offset,
            },
            Instruction::CompareLogicalWord {
                a: 0,
                b: OBJECT_TYPE,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, find_advance);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(find_done);
        self.bind_label(find_advance);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 4,
            offset: 0,
        });
        self.bind_label(find_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, find_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.bind_label(find_done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, payload_found);

        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: PAYLOAD_SLOT,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.make_item_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.make_item_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, make_item_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(make_done);
        self.bind_label(make_item_succeeded);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::StoreWord {
                s: OBJECT_TYPE,
                a: 3,
                offset: shape.type_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: OBJECT_TYPE,
                offset: shape.size_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: shape.node_header_size,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.make_list_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.make_list_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, make_list_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(make_done);
        self.bind_label(make_list_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.bind_label(make_done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, payload_created);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(payload_created);
        self.output
            .instructions
            .push(Instruction::load_immediate(CREATED, 1));
        let after_created_flag = self.fresh_label();
        self.emit_branch_to(after_created_flag);
        self.bind_label(payload_found);
        self.output
            .instructions
            .push(Instruction::load_immediate(CREATED, 0));
        self.bind_label(after_created_flag);

        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::move_register(4, OUTPUT),
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: shape.list_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.make_item_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.make_item_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, object_item_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(object_item_succeeded);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::CompareWordImmediate {
                a: CREATED,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: PAYLOAD_SLOT,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: shape.node_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 4,
                a: 3,
                offset: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, final_callback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 12,
                a: OBJECT_TYPE,
                offset: shape.callback_offset,
            },
            Instruction::load_immediate(4, shape.create_event),
            Instruction::LoadWord {
                d: 3,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::load_immediate(5, 0),
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ]);
        self.bind_label(final_callback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 12,
                a: OBJECT_TYPE,
                offset: shape.callback_offset,
            },
            Instruction::move_register(5, ARGUMENT),
            Instruction::LoadWord {
                d: 3,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::load_immediate(4, shape.ready_event),
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ]);

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::LoadWord {
                d: CREATED,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: OBJECT_TYPE,
                a: 1,
                offset: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord {
                d: ARGUMENT,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: OUTPUT,
                a: 1,
                offset: 24,
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
