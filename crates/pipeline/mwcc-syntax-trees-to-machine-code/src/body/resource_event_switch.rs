//! Resource allocation/free event switches with one retained object pointer.
//!
//! The selector dispatch, initialization stores, lifecycle calls, and boolean
//! exits are one measured scheduling region in legacy linkage-first builds.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{classify, ResourceEventSwitch};

impl Generator {
    pub(crate) fn try_resource_event_switch(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
            || self.lookup_general(&function.parameters[0].name) != Some(3)
            || self.lookup_general(&function.parameters[1].name) != Some(4)
            || [&shape.take, &shape.close, &shape.free]
                .iter()
                .any(|callee| {
                    self.locations.contains_key(callee.as_str())
                        || self.globals.contains_key(callee.as_str())
                })
        {
            return Ok(false);
        }
        self.emit_resource_event_switch(&shape);
        Ok(true)
    }

    fn emit_resource_event_switch(&mut self, shape: &ResourceEventSwitch) {
        const OBJECT: u8 = 31;
        const SELECTOR: u8 = 4;

        let upper_dispatch = self.fresh_label();
        let initialize = self.fresh_label();
        let destroy = self.fresh_label();
        let default_failure = self.fresh_label();
        let success = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![OBJECT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: OBJECT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: OBJECT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, destroy);
        self.emit_branch_conditional_to(4, 0, upper_dispatch);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 2,
            });
        self.emit_branch_conditional_to(4, 0, initialize);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 0, success);
        self.emit_branch_to(default_failure);

        self.bind_label(upper_dispatch);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 5,
            });
        self.emit_branch_conditional_to(4, 0, default_failure);
        self.emit_branch_to(success);

        self.bind_label(initialize);
        let (flags_high, flags_low) = split_address(shape.allocation_flags);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: OBJECT,
                offset: shape.size_offset,
            },
            Instruction::load_immediate_shifted(4, flags_high),
            Instruction::AddImmediate {
                d: 3,
                a: OBJECT,
                immediate: shape.buffer_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: OBJECT,
                offset: shape.position_offset,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: flags_low,
            },
            Instruction::StoreWord {
                s: 0,
                a: OBJECT,
                offset: shape.data_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.take);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.take.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(destroy);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: OBJECT,
            immediate: shape.info_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.close);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.close.clone(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: OBJECT,
            immediate: shape.buffer_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.free);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.free.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(default_failure);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: OBJECT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
