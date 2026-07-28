//! Compose a retained callback-or-fallback helper into resource opening.
//!
//! The object constructor, retained helper, success publication, and failure
//! release share one saved-register and branch schedule in MWCC.  This owner
//! validates the complete semantic transaction before emitting that schedule.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, InlinedCallbackOpen};

impl Generator {
    pub(crate) fn try_inlined_callback_open(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function, &self.inline_bodies) else {
            return Ok(false);
        };
        if !self.globals.contains_key(&shape.callback)
            || !self.addressable_globals.contains_key(&shape.object_type)
            || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_inlined_callback_open(&shape);
        Ok(true)
    }

    fn emit_inlined_callback_open(&mut self, shape: &InlinedCallbackOpen) {
        const OUTPUT: u8 = 29;
        const KIND: u8 = 30;
        const NAME: u8 = 31;
        let made = self.fresh_label();
        let fallback = self.fresh_label();
        let opened = self.fresh_label();
        let failed = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![NAME, KIND, OUTPUT];
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(6, &shape.object_type);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.record_relocation(RelocationKind::Addr16Lo, &shape.object_type);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 6,
                immediate: 0,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: NAME,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: NAME,
                a: 5,
                immediate: 0,
            },
            Instruction::move_register(5, 0),
            Instruction::StoreWord {
                s: KIND,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: KIND,
                a: 4,
                immediate: 0,
            },
            Instruction::load_immediate(4, 0),
            Instruction::StoreWord {
                s: OUTPUT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: OUTPUT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.make);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.make.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, made);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(made);
        self.record_relocation(RelocationKind::EmbSda21, &shape.callback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, fallback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::AddImmediate {
                d: 3,
                a: NAME,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: shape.info_offset,
            },
            Instruction::BranchToLinkRegisterAndLink,
        ]);
        self.emit_branch_to(opened);

        self.bind_label(fallback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: NAME,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: shape.info_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.fallback);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.fallback.clone(),
        });

        self.bind_label(opened);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, failed);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::load_immediate(3, 1),
            Instruction::StoreWord {
                s: KIND,
                a: 4,
                offset: shape.kind_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: shape.length_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.size_offset,
            },
            Instruction::LoadWord {
                d: 4,
                a: OUTPUT,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: shape.info_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.data_offset,
            },
        ]);
        self.emit_branch_to(epilogue);

        self.bind_label(failed);
        self.output
            .instructions
            .push(Instruction::move_register(3, OUTPUT));
        self.record_relocation(RelocationKind::Rel24, &shape.free);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.free.clone(),
        });
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
                d: NAME,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: KIND,
                a: 1,
                offset: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord {
                d: OUTPUT,
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
