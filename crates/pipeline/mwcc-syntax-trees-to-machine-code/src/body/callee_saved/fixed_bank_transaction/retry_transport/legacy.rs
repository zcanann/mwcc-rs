//! Legacy lifetime schedule for the verified transport protocol.
//!
//! r28 retains the interrupt token; r30/r29 retain selected/poll addresses.
//! The stream arguments use r26/r27 for buffer/size and r25/r24 for
//! command/rounded length. After the stream finishes, those dead homes hold
//! status errors and the mailbox command. Each inlined status occurrence has
//! a distinct command slot, while all three share the caller's output word.

use super::recognize::RetryTransport;
use super::*;

struct RetryFrame {
    size: i16,
    saved: i16,
    output: i16,
    command: [i16; 3],
    mailbox: i16,
    patched: bool,
}
impl RetryFrame {
    fn new(patched: bool) -> Self {
        let shift = if patched { 8 } else { 0 };
        Self {
            size: 120 - shift,
            saved: 88 - shift,
            output: 84 - shift,
            command: [80 - shift, 76 - shift, 60 - shift],
            mailbox: 68 - shift,
            patched,
        }
    }
}

impl Generator {
    pub(super) fn emit_legacy_retry_transport(
        &mut self,
        caller: &RetryTransport<'_>,
        status: &Transaction<'_>,
        mailbox: &Transaction<'_>,
        command: u16,
        (begin, end, high): (u8, u8, u16),
    ) {
        let frame = RetryFrame::new(
            self.behavior.plain_linkage_epilogue_style
                == PlainLinkageEpilogueStyle::StackRestoreBeforeReload,
        );
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.frame_size = frame.size;
        self.callee_saved = (24..=31).rev().collect();
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
                offset: -frame.size,
            },
            Instruction::StoreMultipleWord {
                s: 24,
                a: 1,
                offset: frame.saved,
            },
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 27,
                a: 4,
                immediate: 0,
            },
        ]);
        self.bank_transfer_call(caller.acquire);
        let (page, _) = crate::expressions::split_address(status.address);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 28,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate_shifted(25, page),
            Instruction::load_immediate_shifted(31, command as i16),
        ]);
        let first = self.output.instructions.len();
        self.retry_status_read(status, 31, frame.command[0], frame.output, 24, true, false);
        self.retry_busy_test(frame.output, caller.busy_mask, first);
        self.retry_counter_step_and_stream(caller);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(31, command as i16));
        let second = self.output.instructions.len();
        self.retry_status_read(status, 31, frame.command[1], frame.output, 26, false, false);
        self.retry_busy_test(frame.output, caller.busy_mask, second);
        self.retry_counter_load(caller.counter, 0);
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: caller.sequence_shift,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: caller.message_tag,
            },
            Instruction::Or { a: 0, s: 0, b: 27 },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin,
                end,
            },
            Instruction::OrImmediateShifted {
                a: 24,
                s: 0,
                immediate: high,
            },
        ]);
        let write = self.output.instructions.len();
        self.retry_bank_start(mailbox, 24, frame.mailbox, 4, false);
        self.retry_normalize_failure(3);
        self.retry_retained_bank_poll(mailbox);
        self.retry_bank_reset(mailbox, true);
        self.retry_branch(4, write);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(27, command as i16));
        let last = self.output.instructions.len();
        self.retry_status_read(status, 27, frame.command[2], frame.output, 26, false, true);
        self.retry_branch(4, last);
        self.retry_busy_test(frame.output, caller.busy_mask, last);
        self.output
            .instructions
            .push(Instruction::move_register(3, 28));
        self.bank_transfer_call(caller.release);
        if frame.patched {
            self.output
                .instructions
                .push(Instruction::load_immediate(3, caller.result));
        }
        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: 24,
                a: 1,
                offset: frame.saved,
            });
        if !frame.patched {
            self.output.instructions.extend([
                Instruction::load_immediate(3, caller.result),
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: frame.size + 4,
                },
            ]);
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: frame.size,
        });
        if frame.patched {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            });
        }
        self.output.instructions.extend([
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }

    fn retry_counter_step_and_stream(&mut self, caller: &RetryTransport<'_>) {
        self.retry_counter_load(caller.counter, 3);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.record_relocation(RelocationKind::EmbSda21, caller.counter);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.retry_counter_load(caller.counter, 0);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 0,
            s: 0,
            begin: caller.counter_mask.0,
            end: caller.counter_mask.1,
        });
        let false_branch = self.output.instructions.len();
        self.retry_branch(12, 0);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, caller.offset));
        let merge_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let false_target = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        let merge = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[false_branch]
        {
            *target = false_target;
        }
        if let Instruction::Branch { target } = &mut self.output.instructions[merge_branch] {
            *target = merge;
        }
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 27,
                immediate: caller.rounding_bias,
            },
            Instruction::OrImmediateShifted {
                a: 25,
                s: 3,
                immediate: (caller.stream_base >> 16) as u16,
            },
            Instruction::AndContiguousMask {
                a: 24,
                s: 0,
                begin: caller.rounding_mask.0,
                end: caller.rounding_mask.1,
            },
            Instruction::OrImmediate {
                a: 25,
                s: 25,
                immediate: caller.stream_base as u16,
            },
        ]);
        let retry = self.output.instructions.len();
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 25,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 26,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 24,
                immediate: 0,
            },
        ]);
        self.bank_transfer_call(caller.stream);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.retry_branch(12, retry);
    }

    fn retry_status_read(
        &mut self,
        bank: &Transaction<'_>,
        command: u8,
        home: i16,
        output: i16,
        error: u8,
        first: bool,
        retain_result: bool,
    ) {
        self.retry_bank_start(bank, command, home, 2, first);
        // The legacy optimizer leaves this first failure normalization even
        // when the caller discards the transaction result in phases one/two.
        self.retry_normalize_failure(error);
        if first {
            let (_, low) = crate::expressions::split_address(bank.address);
            let poll = self.output.instructions.len();
            self.output.instructions.extend([
                Instruction::AddImmediate {
                    d: 29,
                    a: 25,
                    immediate: low,
                },
                Instruction::LoadWordWithUpdate {
                    d: 0,
                    a: 29,
                    offset: bank.poll,
                },
            ]);
            self.bank_poll_test(bank, poll);
        } else {
            self.retry_retained_bank_poll(bank);
        }
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: output,
            },
            Instruction::load_immediate(4, 4),
            Instruction::load_immediate(5, 0),
        ]);
        self.bank_transfer_call(bank.transfer);
        if retain_result {
            self.retry_normalize_failure(0);
            self.output.instructions.push(Instruction::Or {
                a: 3,
                s: error,
                b: 0,
            });
        }
        self.retry_retained_bank_poll(bank);
        self.retry_bank_reset(bank, retain_result);
    }

    fn retry_bank_start(
        &mut self,
        bank: &Transaction<'_>,
        command: u8,
        home: i16,
        length: i16,
        first: bool,
    ) {
        if first {
            let (_, low) = crate::expressions::split_address(bank.address);
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 0,
                    a: 25,
                    offset: low + bank.selected,
                },
                Instruction::AddImmediate {
                    d: 30,
                    a: 25,
                    immediate: low,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 1,
                    immediate: home,
                },
                Instruction::AndImmediateRecord {
                    a: 0,
                    s: 0,
                    immediate: bank.preserve,
                },
                Instruction::load_immediate(4, length),
                Instruction::load_immediate(5, 1),
                Instruction::OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: bank.insert,
                },
                Instruction::StoreWordWithUpdate {
                    s: 0,
                    a: 30,
                    offset: bank.selected,
                },
            ]);
        } else {
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 0,
                    a: 30,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 1,
                    immediate: home,
                },
                Instruction::load_immediate(4, length),
                Instruction::AndImmediateRecord {
                    a: 0,
                    s: 0,
                    immediate: bank.preserve,
                },
                Instruction::load_immediate(5, 1),
                Instruction::OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: bank.insert,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 30,
                    offset: 0,
                },
            ]);
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: command,
            a: 1,
            offset: home,
        });
        self.bank_transfer_call(bank.transfer);
    }
    fn retry_normalize_failure(&mut self, result: u8) {
        self.output.instructions.extend([
            Instruction::CountLeadingZeros { a: 0, s: 3 },
            Instruction::ShiftRightLogicalImmediate {
                a: result,
                s: 0,
                shift: 5,
            },
        ]);
    }
    fn retry_retained_bank_poll(&mut self, bank: &Transaction<'_>) {
        let poll = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 29,
            offset: 0,
        });
        self.bank_poll_test(bank, poll);
    }
    fn retry_bank_reset(&mut self, bank: &Transaction<'_>, compare: bool) {
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 30,
                offset: 0,
            },
            Instruction::AndImmediateRecord {
                a: 0,
                s: 0,
                immediate: bank.preserve,
            },
        ]);
        if compare {
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
    }
    fn retry_busy_test(&mut self, output: i16, mask: (u8, u8), retry: usize) {
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: output,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: mask.0,
                end: mask.1,
            },
        ]);
        self.retry_branch(4, retry);
    }
    fn retry_branch(&mut self, options: u8, target: usize) {
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit: 2,
                target,
            });
    }
    fn retry_counter_load(&mut self, counter: &str, d: u8) {
        self.record_relocation(RelocationKind::EmbSda21, counter);
        self.output
            .instructions
            .push(Instruction::LoadByteZero { d, a: 0, offset: 0 });
    }
}
