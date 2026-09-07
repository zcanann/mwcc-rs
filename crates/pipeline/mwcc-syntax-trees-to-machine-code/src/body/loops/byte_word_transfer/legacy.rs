//! Legacy loop topology, transaction address lifetimes, and leaf frame layout.
use super::*;
use mwcc_machine_code::Instruction::*;
use packets::addi;

impl Generator {
    pub(super) fn emit_legacy_byte_word_transfer(
        &mut self,
        plan: &Transfer<'_>,
        mixed_types: bool,
    ) {
        let patched =
            self.behavior.byte_word_transfer_style == ByteWordTransferStyle::LegacyInterleaved;
        let (frame, index, word, buffer) = if patched {
            (64, 31, 12, 11)
        } else {
            (72, 29, 30, 31)
        };
        let (high, low) = crate::expressions::split_address(plan.address);
        let data_offset = low + plan.data_offset;
        let control = self.fresh_label();
        let pack_store = self.fresh_label();
        let pack_body = self.fresh_label();
        let pack_tail_setup = self.fresh_label();
        let pack_tail = self.fresh_label();
        let pack_tail_body = self.fresh_label();
        let poll = self.fresh_label();
        let read_body = self.fresh_label();
        let read_tail = self.fresh_label();
        let read_tail_body = self.fresh_label();
        let result = self.fresh_label();
        let epilogue = self.fresh_label();
        self.frame_size = frame;
        self.callee_saved = (22..32).collect();
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame,
            },
            CompareLogicalWordImmediate { a: 5, immediate: 0 },
            StoreMultipleWord {
                s: 22,
                a: 1,
                offset: frame - 40,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, control);
        if mixed_types {
            self.output.instructions.extend([
                addi(index, 0, 0),
                CompareWord { a: index, b: 4 },
                addi(word, 0, 0),
            ]);
            self.emit_branch_conditional_to(4, 0, pack_store);
        } else {
            self.output.instructions.extend([
                CompareWordImmediate { a: 4, immediate: 0 },
                addi(word, 0, 0),
                addi(index, 0, 0),
            ]);
            self.emit_branch_conditional_to(4, 1, pack_store);
        }
        self.output
            .instructions
            .extend([CompareWordImmediate { a: 4, immediate: 8 }, addi(6, 4, -8)]);
        self.emit_branch_conditional_to(4, 1, pack_tail_setup);
        self.output.instructions.push(addi(0, 6, 7));
        if patched {
            self.output
                .instructions
                .push(CompareWordImmediate { a: 6, immediate: 0 });
        }
        self.output.instructions.push(ShiftRightLogicalImmediate {
            a: 0,
            s: 0,
            shift: 3,
        });
        if !patched {
            self.output
                .instructions
                .push(CompareWordImmediate { a: 6, immediate: 0 });
        }
        self.output
            .instructions
            .extend([MoveToCountRegister { s: 0 }, addi(buffer, 3, 0)]);
        self.emit_branch_conditional_to(4, 1, pack_tail_setup);
        self.bind_label(pack_body);
        self.output.instructions.extend(packets::pack(patched));
        self.emit_branch_conditional_to(16, 0, pack_body);
        self.emit_branch_to(pack_tail_setup);
        self.bind_label(pack_tail);
        self.output.instructions.extend([
            SubtractFrom {
                d: 0,
                a: index,
                b: 4,
            },
            CompareWord { a: index, b: 4 },
            MoveToCountRegister { s: 0 },
        ]);
        self.emit_branch_conditional_to(4, 0, pack_store);
        self.bind_label(pack_tail_body);
        self.output.instructions.extend([
            SubtractFromImmediate {
                d: 0,
                a: index,
                immediate: 3,
            },
            LoadByteZero {
                d: 6,
                a: 7,
                offset: 0,
            },
            ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 3,
            },
        ]);
        if patched {
            self.output.instructions.push(addi(7, 7, 1));
        }
        self.output
            .instructions
            .push(ShiftLeftWord { a: 0, s: 6, b: 0 });
        if patched {
            self.output.instructions.push(addi(index, index, 1));
        }
        self.output.instructions.push(Or {
            a: word,
            s: word,
            b: 0,
        });
        if !patched {
            self.output
                .instructions
                .extend([addi(7, 7, 1), addi(index, index, 1)]);
        }
        self.emit_branch_conditional_to(16, 0, pack_tail_body);
        self.bind_label(pack_store);
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(6, high),
            StoreWord {
                s: word,
                a: 6,
                offset: data_offset,
            },
        ]);
        self.bind_label(control);
        self.output.instructions.extend([
            addi(0, 4, -1),
            Instruction::load_immediate_shifted(6, high),
            ShiftLeftImmediate {
                a: 7,
                s: 5,
                shift: plan.mode_shift,
            },
            addi(8, 6, low),
            OrImmediate {
                a: 6,
                s: 7,
                immediate: plan.start,
            },
            ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: plan.count_shift,
            },
            Or { a: 0, s: 6, b: 0 },
            StoreWordWithUpdate {
                s: 0,
                a: 8,
                offset: plan.control_offset,
            },
        ]);
        self.bind_label(poll);
        self.output.instructions.extend([
            LoadWord {
                d: 0,
                a: 8,
                offset: 0,
            },
            AndMaskRecord {
                a: 0,
                s: 0,
                begin: plan.poll_bit,
                end: plan.poll_bit,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, poll);
        self.output
            .instructions
            .push(CompareLogicalWordImmediate { a: 5, immediate: 0 });
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
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(5, high));
            if patched {
                self.output
                    .instructions
                    .push(CompareWordImmediate { a: 4, immediate: 0 });
            }
            self.output.instructions.push(LoadWord {
                d: 0,
                a: 5,
                offset: data_offset,
            });
            if !patched {
                self.output
                    .instructions
                    .push(CompareWordImmediate { a: 4, immediate: 0 });
            }
            self.output.instructions.push(addi(5, 0, 0));
            self.emit_branch_conditional_to(4, 1, result);
        }
        self.output
            .instructions
            .extend([CompareWordImmediate { a: 4, immediate: 8 }, addi(7, 4, -8)]);
        self.emit_branch_conditional_to(4, 1, read_tail);
        self.output.instructions.push(addi(6, 7, 7));
        if patched {
            self.output
                .instructions
                .push(CompareWordImmediate { a: 7, immediate: 0 });
        }
        self.output.instructions.push(ShiftRightLogicalImmediate {
            a: 6,
            s: 6,
            shift: 3,
        });
        if !patched {
            self.output
                .instructions
                .push(CompareWordImmediate { a: 7, immediate: 0 });
        }
        self.output.instructions.push(MoveToCountRegister { s: 6 });
        self.emit_branch_conditional_to(4, 1, read_tail);
        self.bind_label(read_body);
        self.output.instructions.extend(packets::unpack(patched));
        self.emit_branch_conditional_to(16, 0, read_body);
        self.bind_label(read_tail);
        self.output.instructions.extend([
            SubtractFrom { d: 6, a: 5, b: 4 },
            CompareWord { a: 5, b: 4 },
            MoveToCountRegister { s: 6 },
        ]);
        self.emit_branch_conditional_to(4, 0, result);
        self.bind_label(read_tail_body);
        self.output.instructions.push(SubtractFromImmediate {
            d: 4,
            a: 5,
            immediate: 3,
        });
        if patched {
            self.output.instructions.push(addi(5, 5, 1));
        }
        self.output.instructions.extend([
            ShiftLeftImmediate {
                a: 4,
                s: 4,
                shift: 3,
            },
            ShiftRightWord { a: 4, s: 0, b: 4 },
            StoreByte {
                s: 4,
                a: 3,
                offset: 0,
            },
            addi(3, 3, 1),
        ]);
        if !patched {
            self.output.instructions.push(addi(5, 5, 1));
        }
        self.emit_branch_conditional_to(16, 0, read_tail_body);
        self.bind_label(result);
        self.output.instructions.push(addi(3, 0, 1));
        self.emit_branch_to(epilogue);
        self.bind_label(pack_tail_setup);
        self.output.instructions.push(Add {
            d: 7,
            a: 3,
            b: index,
        });
        self.emit_branch_to(pack_tail);
        self.bind_label(epilogue);
        self.output.instructions.extend([
            LoadMultipleWord {
                d: 22,
                a: 1,
                offset: frame - 40,
            },
            addi(1, 1, frame),
            BranchToLinkRegister,
        ]);
    }
}
