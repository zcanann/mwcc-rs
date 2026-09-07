//! Mainline transfer topology and ABI. The 4.x eligibility checks deliberately
//! retain the reference's signed-overflow boundary, including its scalar path.
use super::*;
use mwcc_machine_code::Instruction::*;
use packets::{addi, byte, complement, left, merge, right, scale, store};

impl Generator {
    pub(super) fn emit_mainline_byte_word_transfer(
        &mut self,
        plan: &Transfer<'_>,
        mixed_types: bool,
        control_offset: i16,
    ) {
        let style = self.behavior.byte_word_transfer_style;
        let guarded = matches!(
            style,
            ByteWordTransferStyle::GuardedGameCube | ByteWordTransferStyle::GuardedWii
        );
        let mixed_types = mixed_types && !guarded;
        let inline_saves = self.behavior.use_lmw_stmw;
        let (high, low) = crate::expressions::split_address(plan.address);
        let data_offset = low + plan.data_offset;
        let control = self.fresh_label();
        let pack_store = self.fresh_label();
        let pack_body = self.fresh_label();
        let pack_tail = self.fresh_label();
        let pack_tail_body = self.fresh_label();
        let poll = self.fresh_label();
        let read_body = self.fresh_label();
        let read_tail = self.fresh_label();
        let read_tail_body = self.fresh_label();
        let result = self.fresh_label();
        self.frame_size = 32;
        self.callee_saved = (26..32).collect();
        self.non_leaf = !inline_saves;
        self.output.pre_scheduled = true;
        self.output.instructions.push(StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -32,
        });
        let mode_compare = if guarded {
            CompareWordImmediate { a: 5, immediate: 0 }
        } else {
            CompareLogicalWordImmediate { a: 5, immediate: 0 }
        };
        if inline_saves {
            self.output.instructions.extend([
                mode_compare.clone(),
                StoreMultipleWord {
                    s: 26,
                    a: 1,
                    offset: 8,
                },
            ]);
        } else {
            self.output.instructions.extend([
                MoveFromLinkRegister { d: 0 },
                StoreWord {
                    s: 0,
                    a: 1,
                    offset: 36,
                },
                addi(11, 1, 32),
            ]);
            self.record_relocation(RelocationKind::Rel24, "_savegpr_26");
            self.output.instructions.push(BranchAndLink {
                target: "_savegpr_26".into(),
            });
            self.output.instructions.push(mode_compare.clone());
        }
        self.emit_branch_conditional_to(12, 2, control);
        let count_compare = if guarded {
            CompareWordImmediateField {
                crf: 1,
                a: 4,
                immediate: 0,
            }
        } else {
            CompareWordImmediate { a: 4, immediate: 0 }
        };
        if mixed_types {
            self.output.instructions.extend([
                addi(7, 0, 0),
                addi(0, 0, 0),
                CompareWord { a: 7, b: 4 },
            ]);
            self.emit_branch_conditional_to(4, 0, pack_store);
        } else {
            self.output
                .instructions
                .extend([count_compare.clone(), addi(0, 0, 0), addi(7, 0, 0)]);
            self.emit_branch_conditional_to(4, if guarded { 5 } else { 1 }, pack_store);
        }
        self.output
            .instructions
            .extend([CompareWordImmediate { a: 4, immediate: 8 }, addi(9, 4, -8)]);
        self.emit_branch_conditional_to(4, 1, pack_tail);
        if guarded {
            self.emit_byte_word_unroll_guard(pack_tail);
        }
        self.output.instructions.extend([
            addi(8, 9, 7),
            Instruction::move_register(6, 3),
            ShiftRightLogicalImmediate {
                a: 8,
                s: 8,
                shift: 3,
            },
            MoveToCountRegister { s: 8 },
            CompareWordImmediate { a: 9, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 1, pack_tail);
        self.bind_label(pack_body);
        self.output.instructions.extend(mainline_packets::pack());
        self.emit_branch_conditional_to(16, 0, pack_body);
        self.bind_label(pack_tail);
        self.output.instructions.extend([
            SubtractFrom { d: 6, a: 7, b: 4 },
            Add { d: 9, a: 3, b: 7 },
            MoveToCountRegister { s: 6 },
            CompareWord { a: 7, b: 4 },
        ]);
        self.emit_branch_conditional_to(4, 0, pack_store);
        self.bind_label(pack_tail_body);
        self.output.instructions.extend([
            complement(6, 7),
            byte(8, 9, 0),
            scale(6, 6),
            addi(9, 9, 1),
            left(6, 8, 6),
            addi(7, 7, 1),
            merge(0, 6),
        ]);
        self.emit_branch_conditional_to(16, 0, pack_tail_body);
        self.bind_label(pack_store);
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(6, high),
            StoreWord {
                s: 0,
                a: 6,
                offset: data_offset,
            },
        ]);
        self.bind_label(control);
        self.output.instructions.extend([
            ShiftLeftImmediate {
                a: 6,
                s: 5,
                shift: plan.mode_shift,
            },
            addi(0, 4, -1),
            OrImmediate {
                a: 7,
                s: 6,
                immediate: plan.start,
            },
        ]);
        let count_shift = ShiftLeftImmediate {
            a: 0,
            s: 0,
            shift: plan.count_shift,
        };
        if guarded {
            self.output.instructions.push(count_shift.clone());
        }
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, high));
        if !guarded {
            self.output.instructions.push(count_shift);
        }
        self.output.instructions.extend([
            Or { a: 0, s: 7, b: 0 },
            StoreWord {
                s: 0,
                a: 6,
                offset: control_offset,
            },
        ]);
        // Use the same version policy as standalone fixed-address polling.
        // Compute from layout so changing the frame or packet cannot stale it.
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
        self.bind_label(poll);
        self.output.instructions.extend([
            LoadWord {
                d: 0,
                a: 6,
                offset: control_offset,
            },
            AndMaskRecord {
                a: 0,
                s: 0,
                begin: plan.poll_bit,
                end: plan.poll_bit,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, poll);
        self.output.instructions.push(mode_compare);
        self.emit_branch_conditional_to(4, 2, result);
        if mixed_types {
            self.output.instructions.extend([
                addi(5, 0, 0),
                Instruction::load_immediate_shifted(6, high),
                CompareWord { a: 5, b: 4 },
                LoadWord {
                    d: 0,
                    a: 6,
                    offset: data_offset,
                },
            ]);
            self.emit_branch_conditional_to(4, 0, result);
        } else {
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(5, high),
                count_compare,
                LoadWord {
                    d: 0,
                    a: 5,
                    offset: data_offset,
                },
                addi(5, 0, 0),
            ]);
            self.emit_branch_conditional_to(4, if guarded { 5 } else { 1 }, result);
        }
        self.output
            .instructions
            .extend([CompareWordImmediate { a: 4, immediate: 8 }, addi(7, 4, -8)]);
        self.emit_branch_conditional_to(4, 1, read_tail);
        if guarded {
            self.emit_byte_word_unroll_guard(read_tail);
        }
        self.output.instructions.extend([
            addi(6, 7, 7),
            ShiftRightLogicalImmediate {
                a: 6,
                s: 6,
                shift: 3,
            },
            MoveToCountRegister { s: 6 },
            CompareWordImmediate { a: 7, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 1, read_tail);
        self.bind_label(read_body);
        self.output.instructions.extend(match style {
            ByteWordTransferStyle::Mainline => mainline_packets::unpack_mainline(),
            ByteWordTransferStyle::GuardedGameCube => mainline_packets::unpack_gamecube(),
            ByteWordTransferStyle::GuardedWii => mainline_packets::unpack_wii(),
            _ => unreachable!(),
        });
        self.emit_branch_conditional_to(16, 0, read_body);
        self.bind_label(read_tail);
        self.output.instructions.extend([
            SubtractFrom { d: 6, a: 5, b: 4 },
            MoveToCountRegister { s: 6 },
            CompareWord { a: 5, b: 4 },
        ]);
        self.emit_branch_conditional_to(4, 0, result);
        self.bind_label(read_tail_body);
        self.output.instructions.extend([
            complement(4, 5),
            addi(5, 5, 1),
            scale(4, 4),
            right(4, 0, 4),
            store(4, 0),
            addi(3, 3, 1),
        ]);
        self.emit_branch_conditional_to(16, 0, read_tail_body);
        self.bind_label(result);
        if !guarded {
            self.output.instructions.push(addi(3, 0, 1));
        }
        self.output.instructions.push(if inline_saves {
            LoadMultipleWord {
                d: 26,
                a: 1,
                offset: 8,
            }
        } else {
            addi(11, 1, 32)
        });
        if guarded {
            self.output.instructions.push(addi(3, 0, 1));
        }
        if !inline_saves {
            self.record_relocation(RelocationKind::Rel24, "_restgpr_26");
            self.output.instructions.extend([
                BranchAndLink {
                    target: "_restgpr_26".into(),
                },
                LoadWord {
                    d: 0,
                    a: 1,
                    offset: 36,
                },
                MoveToLinkRegister { s: 0 },
            ]);
        }
        self.output
            .instructions
            .extend([addi(1, 1, 32), BranchToLinkRegister]);
    }

    /// CR1 retains the original count-vs-zero result while CR0 is reused by
    /// the eight-byte threshold. INT_MAX follows the scalar path in 4.x.
    fn emit_byte_word_unroll_guard(&mut self, scalar: mwcc_vreg::Label) {
        let join = self.fresh_label();
        self.output.instructions.push(addi(8, 0, 0));
        self.emit_branch_conditional_to(12, 4, join);
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(6, i16::MIN),
            addi(6, 6, -2),
            CompareWord { a: 4, b: 6 },
        ]);
        self.emit_branch_conditional_to(12, 1, join);
        self.output.instructions.push(addi(8, 0, 1));
        self.bind_label(join);
        self.output
            .instructions
            .push(CompareWordImmediate { a: 8, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, scalar);
    }
}
