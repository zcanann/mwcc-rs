use super::*;

impl Generator {
    fn emit_ascii_range_flag(
        &mut self,
        byte: u8,
        zero_before_compare: bool,
        done: mwcc_vreg::Label,
    ) {
        if zero_before_compare {
            self.load_integer_constant(0, 0);
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: byte,
                immediate: 97,
            });
        if !zero_before_compare {
            self.load_integer_constant(0, 0);
        }
        self.emit_branch_conditional_to(12, 0, done); // blt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: byte,
                immediate: 122,
            });
        self.emit_branch_conditional_to(12, 1, done); // bgt
        self.load_integer_constant(0, 1);
    }

    fn emit_flagged_pointer_adjust(&mut self, pointer: u8, done: mwcc_vreg::Label) {
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: 0 });
        self.emit_branch_conditional_to(12, 2, done); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: pointer,
            a: pointer,
            immediate: -32,
        });
    }

    /// Lower the measured pointer-adjusting ASCII comparison transaction.
    pub(crate) fn try_ascii_pointer_compare(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = recognize::recognize(function) else {
            return Ok(false);
        };
        let Some(first) = self.lookup_general(plan.first) else {
            return Ok(false);
        };
        let Some(second) = self.lookup_general(plan.second) else {
            return Ok(false);
        };
        if (first, second) != (3, 4) || !self.frame_slots.is_empty() {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let loop_head = self.fresh_label();
        let first_special_done = self.fresh_label();
        let first_adjust_done = self.fresh_label();
        let second_range_done = self.fresh_label();
        let second_adjust_done = self.fresh_label();
        let loop_done = self.fresh_label();

        self.bind_label(loop_head);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: first,
            offset: 0,
        });
        self.load_integer_constant(0, 0);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 5,
                immediate: 122,
            });
        self.emit_branch_conditional_to(12, 2, first_special_done); // beq
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 5,
                immediate: 97,
            });
        self.emit_branch_conditional_to(4, 2, first_special_done); // bne
        self.load_integer_constant(0, 1);
        self.bind_label(first_special_done);
        self.emit_flagged_pointer_adjust(first, first_adjust_done);
        self.bind_label(first_adjust_done);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: second,
            offset: 0,
        });
        self.emit_ascii_range_flag(5, true, second_range_done);
        self.bind_label(second_range_done);
        self.emit_flagged_pointer_adjust(second, second_adjust_done);
        self.bind_label(second_adjust_done);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: first,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_done); // bne
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: second,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_done); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: first,
            a: first,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: second,
            a: second,
            immediate: 1,
        });
        self.emit_branch_to(loop_head);

        self.bind_label(loop_done);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 6,
            a: second,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 5, b: 6 });
        let equal = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, equal); // beq

        let final_first_range_done = self.fresh_label();
        let final_first_adjust_done = self.fresh_label();
        self.emit_ascii_range_flag(5, false, final_first_range_done);
        self.bind_label(final_first_range_done);
        self.emit_flagged_pointer_adjust(first, final_first_adjust_done);
        self.bind_label(final_first_adjust_done);

        let final_second_range_done = self.fresh_label();
        let final_second_adjust_done = self.fresh_label();
        self.emit_ascii_range_flag(6, false, final_second_range_done);
        self.bind_label(final_second_range_done);
        self.emit_flagged_pointer_adjust(second, final_second_adjust_done);
        self.bind_label(final_second_adjust_done);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: first,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: second,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        let greater_or_equal = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, greater_or_equal); // bge
        self.load_integer_constant(3, -1);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(greater_or_equal);
        self.load_integer_constant(3, 1);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(equal);
        self.load_integer_constant(3, 0);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
