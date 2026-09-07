//! Compose discarded read transactions in a packet publication helper.
//!
//! The caller owns the shared page/selected/poll homes. Each read owns its
//! command slot and transfer effects. The outer interrupt query deliberately
//! retains the read calls, matching the measured automatic-inline boundary.

use super::*;
use crate::packet_publication::Publication;
use mwcc_versions::PacketPublicationStyle as Style;

impl Generator {
    pub(crate) fn try_composed_packet_reads(
        &mut self,
        function: &Function,
        publication: &Publication<'_>,
        [ready, preserve, tag, field]: [(u8, u8); 4],
    ) -> bool {
        if !self.behavior.automatic_inlining_enabled {
            return false;
        }
        let Some(first_body) =
            self.expanded_visible_bank_transaction(publication.status, &function.name)
        else {
            return false;
        };
        let Some(second_body) =
            self.expanded_visible_bank_transaction(publication.read, &function.name)
        else {
            return false;
        };
        let Some(first) = recognize::transaction(&first_body, &self.fixed_address_arrays) else {
            return false;
        };
        let Some(second) = recognize::transaction(&second_body, &self.fixed_address_arrays) else {
            return false;
        };
        let (
            Payload::Read {
                high: first_command,
            },
            Payload::Read {
                high: second_command,
            },
        ) = (first.payload, second.payload)
        else {
            return false;
        };
        if !same_bank_configuration(&first, &second)
            || !self.bank_transfer_abi_is_compatible(first.transfer)
            || !self.bank_transfer_abi_is_compatible(second.transfer)
        {
            return false;
        }
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.frame_size = 64;
        self.callee_saved = vec![31, 30, 29, 28];
        let (high, low) = crate::expressions::split_address(first.address);
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::load_immediate(4, 2),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate_shifted(0, first_command as i16),
            Instruction::load_immediate(5, 1),
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 60,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 36,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 56,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 52,
            },
            Instruction::StoreWord {
                s: 28,
                a: 1,
                offset: 48,
            },
            Instruction::load_immediate_shifted(28, high),
            Instruction::AddImmediate {
                d: 31,
                a: 28,
                immediate: low,
            },
        ]);
        self.bank_select_word(&first, 28, 31, 6);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.bank_transfer_call(first.transfer);
        self.discarded_packet_read_tail(&first, true);
        let packet_slot = 40 + publication.index as i16 * 4;
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: packet_slot,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: ready.0,
                end: ready.1,
            },
        ]);
        let mut exits = vec![self.packet_exit(12)];
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 6,
                a: 31,
                offset: 0,
            },
            Instruction::load_immediate_shifted(0, second_command as i16),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 32,
            },
            Instruction::AndImmediateRecord {
                a: 6,
                s: 6,
                immediate: second.preserve,
            },
            Instruction::load_immediate(4, 2),
            Instruction::load_immediate(5, 1),
            Instruction::OrImmediate {
                a: 6,
                s: 6,
                immediate: second.insert,
            },
            Instruction::StoreWord {
                s: 6,
                a: 31,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 32,
            },
        ]);
        self.bank_transfer_call(second.transfer);
        self.discarded_packet_read_tail(&second, false);
        self.emit_packet_fields(publication, packet_slot, preserve, tag, field, &mut exits);
        let end = self.output.instructions.len();
        for index in exits {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[index]
            {
                *target = end;
            }
        }
        self.packet_read_epilogue();
        true
    }

    fn discarded_packet_read_tail(&mut self, plan: &Transaction<'_>, initialize_poll: bool) {
        // The legacy compiler retains normalization of the first transfer's
        // failure even after discarding the helper result. Its second transfer
        // result has no surviving normalization or accumulation.
        self.output.instructions.extend([
            Instruction::CountLeadingZeros { a: 0, s: 3 },
            Instruction::ShiftRightLogicalImmediate {
                a: 29,
                s: 0,
                shift: 5,
            },
        ]);
        let poll = self.output.instructions.len();
        if initialize_poll {
            let (_, low) = crate::expressions::split_address(plan.address);
            self.output.instructions.extend([
                Instruction::AddImmediate {
                    d: 30,
                    a: 28,
                    immediate: low,
                },
                Instruction::LoadWordWithUpdate {
                    d: 0,
                    a: 30,
                    offset: plan.poll,
                },
            ]);
        } else {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 30,
                offset: 0,
            });
        }
        self.bank_poll_test(plan, poll);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 40,
            },
            Instruction::load_immediate(4, 4),
            Instruction::load_immediate(5, 0),
        ]);
        self.bank_transfer_call(plan.transfer);
        let poll = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 0,
        });
        self.bank_poll_test(plan, poll);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 0,
            },
            Instruction::AndImmediateRecord {
                a: 0,
                s: 0,
                immediate: plan.preserve,
            },
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 0,
            },
        ]);
    }

    fn packet_read_epilogue(&mut self) {
        let style = self.behavior.packet_publication_style;
        if style != Style::LegacyPatched {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 68,
            });
        }
        for (d, offset) in [(31, 60), (30, 56), (29, 52), (28, 48)] {
            if d == 29 && style == Style::LegacyLateResult {
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
            }
            self.output
                .instructions
                .push(Instruction::LoadWord { d, a: 1, offset });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        if style == Style::LegacyPatched {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            });
        }
        if style != Style::LegacyLateResult {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}
