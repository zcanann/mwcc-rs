//! Legacy word-stream frames and call/poll/clamp issue order.
use super::*;
use mwcc_machine_code::Instruction::*;

fn addi(d: u8, a: u8, immediate: i16) -> Instruction {
    AddImmediate { d, a, immediate }
}

impl Generator {
    pub(super) fn emit_legacy_bank_stream(
        &mut self,
        plan: &Transaction<'_>,
        command: stream::Command,
        writing: bool,
    ) {
        let patched = self.behavior.fixed_bank_stream_style
            == mwcc_versions::FixedBankStreamStyle::LegacySeparateCommand;
        let frame = if patched { 56 } else { 64 };
        let payload = if patched { 24 } else { 36 };
        let word = payload - 4;
        let (high, low) = crate::expressions::split_address(plan.address);
        self.frame_size = frame;
        self.callee_saved = (26..32).collect();
        self.output.instructions.extend([
            MoveFromLinkRegister { d: 0 },
            StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
        ]);
        if patched {
            let mask = AndContiguousMask {
                a: 0,
                s: if command.shift_first { 0 } else { 3 },
                begin: command.mask.0,
                end: command.mask.1,
            };
            let shift = ShiftLeftImmediate {
                a: 0,
                s: if command.shift_first { 3 } else { 0 },
                shift: command.shift,
            };
            self.output.instructions.extend(if command.shift_first {
                [shift, mask]
            } else {
                [mask, shift]
            });
        } else {
            self.output.instructions.push(RotateAndMask {
                a: 0,
                s: 3,
                shift: command.shift,
                begin: command.fused_mask.0,
                end: command.fused_mask.1,
            });
        }
        if patched {
            self.output.instructions.push(StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame,
            });
        }
        self.output.instructions.push(OrImmediateShifted {
            a: 0,
            s: 0,
            immediate: command.high,
        });
        if !patched {
            self.output.instructions.push(StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame,
            });
        }
        // r26 cursor, r27 error, r28 polling slot, r29 bank page,
        // r30 remaining bytes, r31 selected slot; both words live in the frame.
        self.output.instructions.extend([
            StoreMultipleWord {
                s: 26,
                a: 1,
                offset: frame - 24,
            },
            Instruction::load_immediate_shifted(29, high),
            addi(30, 5, 0),
            addi(26, 4, 0),
            addi(31, 29, low),
            addi(3, 1, payload),
            addi(4, 0, 4),
            addi(5, 0, 1),
        ]);
        self.bank_select_word(plan, 29, 31, 6);
        self.output.instructions.push(StoreWord {
            s: 0,
            a: 1,
            offset: payload,
        });
        self.bank_transfer_call(plan.transfer);
        self.output.instructions.extend([
            CountLeadingZeros { a: 0, s: 3 },
            ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            },
            Instruction::move_register(27, 0),
        ]);
        let first_poll = self.output.instructions.len();
        self.output.instructions.extend([
            addi(28, 29, low),
            LoadWordWithUpdate {
                d: 0,
                a: 28,
                offset: plan.poll,
            },
        ]);
        self.bank_poll_test(plan, first_poll);
        let loop_test = self.fresh_label();
        let loop_body = self.fresh_label();
        self.emit_branch_to(loop_test);
        self.bind_label(loop_body);
        if writing {
            self.output.instructions.push(LoadWord {
                d: 0,
                a: 26,
                offset: 0,
            });
        }
        self.output
            .instructions
            .extend([addi(3, 1, word), addi(4, 0, 4)]);
        if writing {
            if patched {
                self.output.instructions.push(addi(26, 26, 4));
            }
            self.output.instructions.push(StoreWord {
                s: 0,
                a: 1,
                offset: word,
            });
        }
        self.output
            .instructions
            .push(addi(5, 0, i16::from(writing)));
        if writing && !patched {
            self.output.instructions.push(addi(26, 26, 4));
        }
        self.bank_transfer_call(plan.transfer);
        self.output.instructions.extend([
            CountLeadingZeros { a: 0, s: 3 },
            ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            },
            Or { a: 27, s: 27, b: 0 },
        ]);
        let poll = self.output.instructions.len();
        self.output.instructions.push(LoadWord {
            d: 0,
            a: 28,
            offset: 0,
        });
        self.bank_poll_test(plan, poll);
        if !writing {
            self.output.instructions.push(LoadWord {
                d: 0,
                a: 1,
                offset: word,
            });
        }
        self.output.instructions.push(AddImmediateCarryingRecord {
            d: 30,
            a: 30,
            immediate: -4,
        });
        if !writing {
            self.output.instructions.extend([
                StoreWord {
                    s: 0,
                    a: 26,
                    offset: 0,
                },
                addi(26, 26, 4),
            ]);
        }
        self.emit_branch_conditional_to(4, 0, loop_test);
        self.output.instructions.push(addi(30, 0, 0));
        self.bind_label(loop_test);
        self.output.instructions.push(CompareWordImmediate {
            a: 30,
            immediate: 0,
        });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.bank_reset_and_result(plan, 31, 27);
        self.output.instructions.push(LoadMultipleWord {
            d: 26,
            a: 1,
            offset: frame - 24,
        });
        let stack = addi(1, 1, frame);
        let reload = LoadWord {
            d: 0,
            a: 1,
            offset: if patched { 4 } else { frame + 4 },
        };
        self.output.instructions.extend(if patched {
            [stack, reload]
        } else {
            [reload, stack]
        });
        self.output
            .instructions
            .extend([MoveToLinkRegister { s: 0 }, BranchToLinkRegister]);
    }
}
