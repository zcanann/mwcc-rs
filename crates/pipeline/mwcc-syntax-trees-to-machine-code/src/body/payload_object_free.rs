//! Release an object whose allocation header owns its list and event type.
//!
//! The output slot and payload survive an indirect destructor callback. Legacy
//! MWCC then rewinds the published object pointer before calling the list
//! remover and clears it only after successful removal.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, PayloadObjectFree};

impl Generator {
    pub(crate) fn try_payload_object_free(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_payload_object_free(&shape);
        Ok(true)
    }

    fn emit_payload_object_free(&mut self, shape: &PayloadObjectFree) {
        const OUTPUT: u8 = 30;
        const PAYLOAD: u8 = 31;
        let failure = self.fresh_label();
        let removed = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![PAYLOAD, OUTPUT];
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
                offset: -24,
            },
            Instruction::StoreWord {
                s: PAYLOAD,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: OUTPUT,
                a: 1,
                offset: 16,
            },
            Instruction::OrRecord {
                a: OUTPUT,
                s: 3,
                b: 3,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: PAYLOAD,
                a: 3,
                offset: -shape.header_size,
            },
            Instruction::load_immediate(4, shape.event),
            Instruction::load_immediate(5, 0),
            Instruction::LoadWord {
                d: 6,
                a: PAYLOAD,
                offset: shape.type_offset,
            },
            Instruction::LoadWord {
                d: 12,
                a: 6,
                offset: shape.callback_offset,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::LoadWord {
                d: 3,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: OUTPUT,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -shape.header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: PAYLOAD,
                offset: shape.list_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.free_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.free_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, removed);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(removed);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::load_immediate(3, 1),
        ]);
        self.emit_branch_to(epilogue);
        self.bind_label(failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: PAYLOAD,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: OUTPUT,
                a: 1,
                offset: 16,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
