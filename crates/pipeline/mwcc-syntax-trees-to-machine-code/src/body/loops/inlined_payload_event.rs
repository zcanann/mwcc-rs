//! Compose a retained payload/type predicate into an event dispatcher.
//!
//! The outer registry test and the inlined predicate deliberately repeat the
//! registry lookup. Legacy MWCC preserves both calls, keeps the object, event,
//! argument, payload, and requested type in one saved-register range, then
//! returns the indirect callback result directly.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, classify_predicate, InlinedPayloadEvent};

impl Generator {
    pub(crate) fn try_inlined_payload_event(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let Some(predicate) = self
            .inline_bodies
            .retained_body(&shape.helper)
            .and_then(classify_predicate)
        else {
            return Ok(false);
        };
        if predicate.registry != shape.registry
            || predicate.test_callee != shape.test_callee
            || predicate.header_size != shape.header_size
            || predicate.type_offset != shape.type_offset
            || !self.globals.contains_key(&shape.registry)
            || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        self.emit_inlined_payload_event(&shape);
        Ok(true)
    }

    fn emit_inlined_payload_event(&mut self, shape: &InlinedPayloadEvent) {
        const OBJECT: u8 = 26;
        const EVENT: u8 = 27;
        const ARGUMENT: u8 = 28;
        const PAYLOAD: u8 = 29;
        const REQUESTED_TYPE: u8 = 30;
        const HEADER_ADDRESS: u8 = 31;
        let failure = self.fresh_label();
        let predicate_failure = self.fresh_label();
        let predicate_done = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 48;
        self.callee_saved = (OBJECT..=HEADER_ADDRESS).collect();
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
                offset: -48,
            },
            Instruction::StoreMultipleWord {
                s: OBJECT,
                a: 1,
                offset: 24,
            },
            Instruction::OrRecord {
                a: OBJECT,
                s: 3,
                b: 3,
            },
            Instruction::AddImmediate {
                d: EVENT,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: ARGUMENT,
                a: 5,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.push(Instruction::AddImmediate {
            d: HEADER_ADDRESS,
            a: OBJECT,
            immediate: -shape.header_size,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.registry);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: PAYLOAD,
                a: OBJECT,
                offset: -shape.header_size,
            },
            Instruction::move_register(4, PAYLOAD),
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.test_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.test_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.extend([
            Instruction::CompareLogicalWordImmediate {
                a: OBJECT,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: REQUESTED_TYPE,
                a: PAYLOAD,
                offset: shape.type_offset,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, predicate_failure);
        self.output.instructions.push(Instruction::LoadWord {
            d: HEADER_ADDRESS,
            a: HEADER_ADDRESS,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.registry);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::move_register(4, HEADER_ADDRESS),
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.test_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.test_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, predicate_failure);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: HEADER_ADDRESS,
                offset: shape.type_offset,
            },
            Instruction::CompareLogicalWord {
                a: 0,
                b: REQUESTED_TYPE,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, predicate_failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(predicate_done);
        self.bind_label(predicate_failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.bind_label(predicate_done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, failure);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: PAYLOAD,
                offset: shape.type_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: OBJECT,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: EVENT,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 12,
                a: 5,
                offset: shape.callback_offset,
            },
            Instruction::AddImmediate {
                d: 5,
                a: ARGUMENT,
                immediate: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ]);
        self.emit_branch_to(epilogue);
        self.bind_label(failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadMultipleWord {
                d: OBJECT,
                a: 1,
                offset: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 52,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 48,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
