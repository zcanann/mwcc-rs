use super::*;

impl Generator {
    /// Lower a byte-class tokenizer as MWCC's single framed transaction.
    pub(crate) fn try_byte_class_tokenizer(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = recognize::recognize(function) else {
            return Ok(false);
        };
        let Some(string) = self.lookup_general(plan.string) else {
            return Ok(false);
        };
        let Some(control) = self.lookup_general(plan.control) else {
            return Ok(false);
        };
        let Some(next_token) = self.lookup_general(plan.next_token) else {
            return Ok(false);
        };
        if (string, control, next_token) != (3, 4, 5) || !self.frame_slots.is_empty() {
            return Ok(false);
        }

        const FRAME_SIZE: i16 = 48;
        const MAP_OFFSET: i16 = 8;

        self.output.pre_scheduled = true;
        self.frame_size = FRAME_SIZE;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -FRAME_SIZE,
            });

        // The source for-loop becomes an unrolled 32-byte clear. MWCC creates
        // the bitmap base after the first store, filling the store latency.
        self.load_integer_constant(0, 0);
        self.load_integer_constant(6, 1);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 1,
            offset: MAP_OFFSET,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 1,
            immediate: MAP_OFFSET,
        });
        for offset in MAP_OFFSET + 1..MAP_OFFSET + 32 {
            self.output
                .instructions
                .push(Instruction::StoreByte { s: 0, a: 1, offset });
        }

        // Build the byte-class bitmap, including the terminating zero byte.
        let build_map = self.fresh_label();
        self.bind_label(build_map);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 10,
            a: control,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: control,
            a: control,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 9,
                s: 10,
                shift: 3,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 10,
                clear: 29,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 7, a: 8, b: 9 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 0, s: 6, b: 0 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 10,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 7, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreByteIndexed { s: 0, a: 8, b: 9 });
        self.emit_branch_conditional_to(4, 2, build_map); // bne

        // Select the explicit string or the caller's continuation pointer.
        let use_continuation = self.fresh_label();
        let selected = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: string,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, use_continuation); // beq
        self.output
            .instructions
            .push(Instruction::move_register(8, string));
        self.emit_branch_to(selected);
        self.bind_label(use_continuation);
        self.output.instructions.push(Instruction::LoadWord {
            d: 8,
            a: next_token,
            offset: 0,
        });
        self.bind_label(selected);

        // Skip leading bytes that belong to the control set.
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 1,
            immediate: MAP_OFFSET,
        });
        self.load_integer_constant(3, 1);
        let skip_body = self.fresh_label();
        let skip_test = self.fresh_label();
        let skip_done = self.fresh_label();
        self.emit_branch_to(skip_test);
        self.bind_label(skip_body);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 1,
        });
        self.bind_label(skip_test);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 7,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 7,
            shift: 29,
            begin: 27,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 7,
                clear: 29,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 4, a: 6, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 0, s: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 4, b: 0 });
        self.emit_branch_conditional_to(12, 2, skip_done); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, skip_body); // bne
        self.bind_label(skip_done);
        self.output
            .instructions
            .push(Instruction::move_register(3, 8));

        // Scan until a delimiter or NUL, replacing a found delimiter with NUL.
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 1,
            immediate: MAP_OFFSET,
        });
        self.load_integer_constant(4, 1);
        let scan_body = self.fresh_label();
        let not_delimiter = self.fresh_label();
        let scan_test = self.fresh_label();
        let scan_done = self.fresh_label();
        self.emit_branch_to(scan_test);
        self.bind_label(scan_body);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 0,
            shift: 29,
            begin: 27,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 29,
            });
        self.output
            .instructions
            .push(Instruction::LoadByteZeroIndexed { d: 6, a: 7, b: 6 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftWord { a: 0, s: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::AndRecord { a: 0, s: 6, b: 0 });
        self.emit_branch_conditional_to(12, 2, not_delimiter); // beq
        self.load_integer_constant(0, 0);
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 1,
        });
        self.emit_branch_to(scan_done);
        self.bind_label(not_delimiter);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 1,
        });
        self.bind_label(scan_test);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, scan_body); // bne
        self.bind_label(scan_done);

        // Store the continuation before nulling the return for an empty token.
        let nonempty = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 3, b: 8 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 8,
            a: next_token,
            offset: 0,
        });
        self.emit_branch_conditional_to(4, 2, nonempty); // bne
        self.load_integer_constant(3, 0);
        self.bind_label(nonempty);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: FRAME_SIZE,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
