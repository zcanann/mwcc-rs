//! Aligned chunked reads through an optional callback or direct fallback.
//!
//! Five values remain live through the read/copy calls. Legacy MWCC allocates
//! them as one r27..r31 window, reuses both the position and callback loads, and
//! advances the destination in place. This owner keeps that loop as one
//! scheduling and lifetime region.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, ChunkedCallbackRead};

impl Generator {
    pub(crate) fn try_chunked_callback_read(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
            || !self.globals.contains_key(&shape.callback)
            || [&shape.fallback, &shape.copy].iter().any(|callee| {
                self.locations.contains_key(callee.as_str())
                    || self.globals.contains_key(callee.as_str())
            })
        {
            return Ok(false);
        }
        self.emit_chunked_callback_read(&shape);
        Ok(true)
    }

    fn emit_chunked_callback_read(&mut self, shape: &ChunkedCallbackRead) {
        const OBJECT: u8 = 27;
        const TARGET: u8 = 28;
        const REMAINING: u8 = 29;
        const USED: u8 = 30;
        const EXTRA: u8 = 31;
        const ALIGNED: u8 = 6;
        const READ_SIZE: u8 = 5;

        let bounded = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_test = self.fresh_label();
        let capped = self.fresh_label();
        let fallback = self.fresh_label();
        let read_complete = self.fresh_label();
        let copied = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 48;
        self.callee_saved = vec![EXTRA, USED, REMAINING, TARGET, OBJECT];
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
                offset: 28,
            },
            Instruction::move_register(OBJECT, 3),
            Instruction::move_register(REMAINING, 5),
            Instruction::AddImmediate {
                d: TARGET,
                a: 4,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: shape.position_offset,
            },
            Instruction::LoadWord {
                d: READ_SIZE,
                a: OBJECT,
                offset: shape.size_offset,
            },
            Instruction::Add {
                d: 0,
                a: 3,
                b: REMAINING,
            },
            Instruction::CompareWord { a: 0, b: READ_SIZE },
        ]);
        self.emit_branch_conditional_to(4, 1, bounded);
        self.output.instructions.push(Instruction::SubtractFrom {
            d: REMAINING,
            a: 3,
            b: READ_SIZE,
        });
        self.bind_label(bounded);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: REMAINING,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, loop_test);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, -1));
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: TARGET,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.emit_branch_to(loop_test);

        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: USED,
                a: REMAINING,
                immediate: 0,
            },
            Instruction::CompareWordImmediate {
                a: USED,
                immediate: shape.chunk_size,
            },
        ]);
        self.emit_branch_conditional_to(4, 1, capped);
        self.output
            .instructions
            .push(Instruction::load_immediate(USED, shape.chunk_size));
        self.bind_label(capped);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: OBJECT,
            offset: shape.position_offset,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.callback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::AndContiguousMask {
                a: EXTRA,
                s: 3,
                begin: 30,
                end: 31,
            },
            Instruction::AddImmediate {
                d: 0,
                a: EXTRA,
                immediate: 31,
            },
            Instruction::Add {
                d: 0,
                a: USED,
                b: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            },
            Instruction::AndContiguousMask {
                a: ALIGNED,
                s: 3,
                begin: 0,
                end: 29,
            },
            Instruction::AndContiguousMask {
                a: READ_SIZE,
                s: 0,
                begin: 0,
                end: 26,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, fallback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: OBJECT,
                offset: shape.data_offset,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::LoadWord {
                d: 4,
                a: OBJECT,
                offset: shape.buffer_offset,
            },
            Instruction::load_immediate(7, 0),
            Instruction::BranchToLinkRegisterAndLink,
        ]);
        self.emit_branch_to(read_complete);

        self.bind_label(fallback);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: OBJECT,
                offset: shape.data_offset,
            },
            Instruction::load_immediate(7, 2),
            Instruction::LoadWord {
                d: 4,
                a: OBJECT,
                offset: shape.buffer_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.fallback);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.fallback.clone(),
        });

        self.bind_label(read_complete);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: OBJECT,
                offset: shape.buffer_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: TARGET,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: USED,
                immediate: 0,
            },
            Instruction::Add {
                d: 4,
                a: 0,
                b: EXTRA,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.copy);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.copy.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, copied);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(copied);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: OBJECT,
                offset: shape.position_offset,
            },
            Instruction::Add {
                d: TARGET,
                a: TARGET,
                b: USED,
            },
            Instruction::SubtractFrom {
                d: REMAINING,
                a: USED,
                b: REMAINING,
            },
            Instruction::Add {
                d: 0,
                a: 0,
                b: USED,
            },
            Instruction::StoreWord {
                s: 0,
                a: OBJECT,
                offset: shape.position_offset,
            },
        ]);
        self.bind_label(loop_test);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: REMAINING,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadMultipleWord {
                d: OBJECT,
                a: 1,
                offset: 28,
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
