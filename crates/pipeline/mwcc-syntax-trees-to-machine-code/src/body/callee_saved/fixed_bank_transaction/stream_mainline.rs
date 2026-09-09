//! Mainline word-stream frames, bank lifetimes, and call/poll issue order.
use super::*;
use mwcc_machine_code::Instruction::*;
use mwcc_versions::FixedBankStreamStyle;

fn addi(d: u32, a: u32, immediate: i16) -> Instruction {
    AddImmediate { d, a, immediate }
}
fn load(d: u32, a: u32, offset: i16) -> Instruction {
    LoadWord { d, a, offset }
}
fn store(s: u32, a: u32, offset: i16) -> Instruction {
    StoreWord { s, a, offset }
}
fn boolean(a: u32) -> Instruction {
    ShiftRightLogicalImmediate { a, s: 0, shift: 5 }
}

impl Generator {
    pub(super) fn emit_mainline_bank_stream(
        &mut self,
        plan: &Transaction<'_>,
        command: stream::Command,
        writing: bool,
        poll_offset: i16,
    ) {
        let style = self.behavior.fixed_bank_stream_style;
        let retained = matches!(
            style,
            FixedBankStreamStyle::RetainedPage | FixedBankStreamStyle::RetainedPageEarlyStore
        );
        let early_store = style == FixedBankStreamStyle::RetainedPageEarlyStore;
        let materialized_poll = writing && style == FixedBankStreamStyle::MainlineImmediateMask;
        let (high, low) = crate::expressions::split_address(plan.address);
        let selected = low
            .checked_add(plan.selected)
            .expect("recognized selected displacement");
        // r31 cursor, r30 remaining bytes, r28 error, r29 repeated-poll page.
        // Four individual saves are retained even with -use_lmw_stmw on.
        self.frame_size = 32;
        self.callee_saved = (28..32).collect();
        self.output.instructions.extend([
            StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            MoveFromLinkRegister { d: 0 },
        ]);
        if !retained {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(7, high));
        }
        self.output.instructions.extend([
            store(0, 1, 36),
            RotateAndMask {
                a: 0,
                s: 3,
                shift: command.shift,
                begin: command.fused_mask.0,
                end: command.fused_mask.1,
            },
            OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: command.high,
            },
        ]);
        if !retained {
            self.output.instructions.push(addi(3, 1, 12));
        }
        self.output.instructions.extend([
            store(31, 1, 28),
            Instruction::move_register(31, 4),
            addi(4, 0, 4),
            store(30, 1, 24),
            Instruction::move_register(30, 5),
            addi(5, 0, 1),
            store(29, 1, 20),
        ]);
        if retained {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(29, high));
        }
        let page = if retained { 29 } else { 7 };
        let value = if retained { 3 } else { 6 };
        self.output.instructions.extend([
            store(28, 1, 16),
            load(value, page, selected),
            AndImmediateRecord {
                a: 6,
                s: value,
                immediate: plan.preserve,
            },
        ]);
        if retained {
            self.output.instructions.push(addi(3, 1, 12));
        }
        self.output.instructions.extend([
            OrImmediate {
                a: 6,
                s: 6,
                immediate: plan.insert,
            },
            store(6, page, selected),
            store(0, 1, 12),
        ]);
        self.bank_transfer_call(plan.transfer);
        self.output
            .instructions
            .push(CountLeadingZeros { a: 0, s: 3 });
        if retained {
            self.output.instructions.push(boolean(28));
        } else {
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(3, high),
                boolean(0),
                Instruction::move_register(28, 0),
            ]);
        }
        if materialized_poll {
            self.output.instructions.push(addi(3, 3, poll_offset));
        }
        let loop_poll_offset = if materialized_poll { 0 } else { poll_offset };
        self.mainline_stream_poll(plan, if retained { 29 } else { 3 }, loop_poll_offset);
        if materialized_poll {
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(3, high),
                addi(29, 3, poll_offset),
            ]);
        } else {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(29, high));
        }
        let loop_test = self.fresh_label();
        let loop_body = self.fresh_label();
        self.emit_branch_to(loop_test);
        self.bind_label(loop_body);
        if writing {
            self.output.instructions.push(load(0, 31, 0));
        }
        self.output.instructions.push(addi(3, 1, 8));
        if writing && early_store {
            self.output.instructions.push(store(0, 1, 8));
        }
        self.output
            .instructions
            .extend([addi(4, 0, 4), addi(5, 0, i16::from(writing))]);
        if writing {
            if !early_store {
                self.output.instructions.push(store(0, 1, 8));
            }
            self.output.instructions.push(addi(31, 31, 4));
        }
        self.bank_transfer_call(plan.transfer);
        self.output.instructions.extend([
            CountLeadingZeros { a: 0, s: 3 },
            boolean(0),
            Or { a: 28, s: 28, b: 0 },
        ]);
        self.mainline_stream_poll(plan, 29, loop_poll_offset);
        if !writing {
            self.output.instructions.push(load(0, 1, 8));
        }
        self.output.instructions.push(AddImmediateCarryingRecord {
            d: 30,
            a: 30,
            immediate: -4,
        });
        if !writing {
            self.output
                .instructions
                .extend([store(0, 31, 0), addi(31, 31, 4)]);
        }
        self.emit_branch_conditional_to(4, 0, loop_test);
        self.output.instructions.push(addi(30, 0, 0));
        self.bind_label(loop_test);
        self.output.instructions.push(CompareWordImmediate {
            a: 30,
            immediate: 0,
        });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.mainline_stream_reset(plan, high, selected);
        self.output.instructions.extend([
            load(0, 1, 36),
            load(31, 1, 28),
            load(30, 1, 24),
            load(29, 1, 20),
            load(28, 1, 16),
            MoveToLinkRegister { s: 0 },
            addi(1, 1, 32),
            BranchToLinkRegister,
        ]);
    }

    fn mainline_stream_poll(&mut self, plan: &Transaction<'_>, page: u32, offset: i16) {
        if let mwcc_versions::FixedAddressPollAddressStyle::FoldedAlignedBankDisplacement {
            alignment,
        } = self.behavior.fixed_address_poll_address_style
        {
            while (self.output.instructions.len() * 4) % usize::from(alignment) != 0 {
                self.output.instructions.push(OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: 0,
                });
            }
        }
        let target = self.output.instructions.len();
        self.output.instructions.push(load(0, page, offset));
        self.bank_poll_test(plan, target);
    }

    fn mainline_stream_reset(&mut self, plan: &Transaction<'_>, high: i16, selected: i16) {
        if self.behavior.fixed_bank_stream_style == FixedBankStreamStyle::MainlineRegisterMask {
            if plan.preserve > i16::MAX as u16 {
                // Build 53 materializes the unsigned mask before normalizing the result.
                self.output.instructions.extend([
                    Instruction::load_immediate_shifted(5, high),
                    Instruction::load_immediate_shifted(3, 1),
                    load(4, 5, selected),
                    addi(3, 3, plan.preserve as i16),
                    CountLeadingZeros { a: 0, s: 28 },
                    And { a: 4, s: 4, b: 3 },
                    store(4, 5, selected),
                    boolean(3),
                ]);
            } else {
                self.output.instructions.extend([
                    Instruction::load_immediate_shifted(6, high),
                    CountLeadingZeros { a: 0, s: 28 },
                    load(5, 6, selected),
                    addi(4, 0, plan.preserve as i16),
                    boolean(3),
                    And { a: 0, s: 5, b: 4 },
                    store(0, 6, selected),
                ]);
            }
        } else {
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(5, high),
                CountLeadingZeros { a: 0, s: 28 },
                load(4, 5, selected),
                boolean(3),
                AndImmediateRecord {
                    a: 0,
                    s: 4,
                    immediate: plan.preserve,
                },
                store(0, 5, selected),
            ]);
        }
    }
}
