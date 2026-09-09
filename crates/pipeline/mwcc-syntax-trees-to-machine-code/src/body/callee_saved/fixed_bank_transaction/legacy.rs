//! Legacy address lifetimes: retained page, selected slot, and repeated poll slot.

use super::*;

impl Generator {
    pub(super) fn emit_legacy_fixed_bank_transaction(&mut self, plan: &Transaction<'_>) {
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        match plan.payload {
            Payload::Write { begin, end, high } => {
                self.emit_legacy_bank_write(plan, begin, end, high)
            }
            Payload::Read { high } => self.emit_legacy_bank_read(plan, high),
            Payload::Stream { command, writing } => {
                self.emit_legacy_bank_stream(plan, command, writing)
            }
        }
    }

    pub(super) fn bank_transfer_call(&mut self, target: &str) {
        self.record_relocation(RelocationKind::Rel24, target);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: target.to_owned(),
        });
    }

    pub(super) fn bank_poll_test(&mut self, plan: &Transaction<'_>, target: usize) {
        self.output.instructions.extend([
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: plan.poll_begin,
                end: plan.poll_end,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target,
            },
        ]);
    }

    pub(super) fn bank_select_word(
        &mut self,
        plan: &Transaction<'_>,
        page: u32,
        selected: u32,
        value: u32,
    ) {
        let (_, low) = crate::expressions::split_address(plan.address);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: value,
                a: page,
                offset: low + plan.selected,
            },
            Instruction::AndImmediateRecord {
                a: value,
                s: value,
                immediate: plan.preserve,
            },
            Instruction::OrImmediate {
                a: value,
                s: value,
                immediate: plan.insert,
            },
            Instruction::StoreWordWithUpdate {
                s: value,
                a: selected,
                offset: plan.selected,
            },
        ]);
    }

    fn bank_select_store(&mut self, plan: &Transaction<'_>, page: u32, selected: u32, payload: i16) {
        let early_mode = self.behavior.plain_linkage_epilogue_style
            == PlainLinkageEpilogueStyle::StackRestoreBeforeReload;
        let value = if early_mode { 6 } else { 5 };
        self.bank_select_word(plan, page, selected, value);
        if !early_mode {
            self.output
                .instructions
                .push(Instruction::load_immediate(5, 1));
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: payload,
        });
        self.bank_transfer_call(plan.transfer);
    }

    pub(super) fn bank_reset_and_result(
        &mut self,
        plan: &Transaction<'_>,
        selected: u32,
        error: u32,
    ) {
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: selected,
                offset: 0,
            },
            Instruction::CountLeadingZeros { a: 0, s: error },
            Instruction::ShiftRightLogicalImmediate {
                a: 3,
                s: 0,
                shift: 5,
            },
            Instruction::AndImmediateRecord {
                a: 0,
                s: 4,
                immediate: plan.preserve,
            },
            Instruction::StoreWord {
                s: 0,
                a: selected,
                offset: 0,
            },
        ]);
    }

    fn emit_legacy_bank_write(
        &mut self,
        plan: &Transaction<'_>,
        begin: u8,
        end: u8,
        payload_high: u16,
    ) {
        let (high, low) = crate::expressions::split_address(plan.address);
        let stack_reload = self.behavior.plain_linkage_epilogue_style
            == PlainLinkageEpilogueStyle::StackRestoreBeforeReload;
        let payload = if stack_reload { 16 } else { 20 };
        self.frame_size = 32;
        self.callee_saved = vec![31, 30];
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::load_immediate(4, 4),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 3,
                begin,
                end,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: payload_high,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
        ]);
        if stack_reload {
            self.output
                .instructions
                .push(Instruction::load_immediate(5, 1));
        }
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::load_immediate_shifted(31, high),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: payload,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 31,
                immediate: low,
            },
        ]);
        self.bank_select_store(plan, 31, 30, payload);
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        let status = Instruction::ShiftRightLogicalImmediate {
            a: 5,
            s: 0,
            shift: 5,
        };
        let base = Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: low,
        };
        self.output.instructions.extend(if stack_reload {
            [base, status]
        } else {
            [status, base]
        });
        let loop_start = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: plan.poll,
        });
        self.bank_poll_test(plan, loop_start);
        self.bank_reset_and_result(plan, 30, 5);
        if !stack_reload {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            });
        }
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 24,
            },
        ]);
        if stack_reload {
            self.output.instructions.extend([
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 32,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::MoveToLinkRegister { s: 0 },
            ]);
        } else {
            let stack = Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            };
            let link = Instruction::MoveToLinkRegister { s: 0 };
            self.output
                .instructions
                .extend(if self.behavior.structured_saved_gpr_stack_first {
                    [stack, link]
                } else {
                    [link, stack]
                });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }

    fn emit_legacy_bank_read(&mut self, plan: &Transaction<'_>, command_high: u16) {
        let (high, low) = crate::expressions::split_address(plan.address);
        let stack_reload = self.behavior.plain_linkage_epilogue_style
            == PlainLinkageEpilogueStyle::StackRestoreBeforeReload;
        let frame = if stack_reload { 48 } else { 56 };
        let payload = if stack_reload { 20 } else { 24 };
        self.frame_size = frame;
        self.callee_saved = vec![27, 28, 29, 30, 31];
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::load_immediate(4, 2),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate_shifted(0, command_high as i16),
        ]);
        if stack_reload {
            self.output
                .instructions
                .push(Instruction::load_immediate(5, 1));
        }
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame,
            },
            Instruction::StoreMultipleWord {
                s: 27,
                a: 1,
                offset: frame - 20,
            },
            Instruction::load_immediate_shifted(30, high),
            Instruction::AddImmediate {
                d: 27,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 30,
                immediate: low,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: payload,
            },
        ]);
        self.bank_select_store(plan, 30, 29, payload);
        self.output.instructions.extend([
            Instruction::CountLeadingZeros { a: 0, s: 3 },
            Instruction::ShiftRightLogicalImmediate {
                a: 31,
                s: 0,
                shift: 5,
            },
        ]);
        let first_poll = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 28,
                a: 30,
                immediate: low,
            },
            Instruction::LoadWordWithUpdate {
                d: 0,
                a: 28,
                offset: plan.poll,
            },
        ]);
        self.bank_poll_test(plan, first_poll);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 27,
                immediate: 0,
            },
            Instruction::load_immediate(4, 4),
            Instruction::load_immediate(5, 0),
        ]);
        self.bank_transfer_call(plan.transfer);
        self.output.instructions.extend([
            Instruction::CountLeadingZeros { a: 0, s: 3 },
            Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            },
            Instruction::Or { a: 3, s: 31, b: 0 },
        ]);
        let second_poll = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 28,
            offset: 0,
        });
        self.bank_poll_test(plan, second_poll);
        self.bank_reset_and_result(plan, 29, 3);
        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: 27,
                a: 1,
                offset: frame - 20,
            });
        let stack = Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: frame,
        };
        let reload = Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: if stack_reload { 4 } else { frame + 4 },
        };
        self.output.instructions.extend(if stack_reload {
            [stack, reload]
        } else {
            [reload, stack]
        });
        self.output.instructions.extend([
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
