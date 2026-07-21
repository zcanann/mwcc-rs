use super::*;

impl Generator {
    fn emit_ascii_hash_iteration(
        &mut self,
        byte: u8,
        accumulator: u8,
        after_shift: Option<Instruction>,
        after_and: Option<Instruction>,
    ) {
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: byte,
                shift: 1,
            });
        if let Some(instruction) = after_shift {
            self.output.instructions.push(instruction);
        }
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: byte,
            b: 0,
        });
        if let Some(instruction) = after_and {
            self.output.instructions.push(instruction);
        }
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: 26,
            end: 26,
        });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: byte,
            a: 0,
            b: byte,
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 0,
                a: accumulator,
                immediate: 131,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: byte,
                s: byte,
                clear: 24,
            });
        self.output.instructions.push(Instruction::Add {
            d: accumulator,
            a: byte,
            b: 0,
        });
    }

    /// Lower the recognized ASCII-folding hash family with MWCC's schedules.
    pub(crate) fn try_ascii_case_fold_hash_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(hash) = recognize(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        self.output.pre_scheduled = true;
        match hash {
            AsciiHashLoop::NullTerminated { pointer } => {
                let Some(pointer) = self
                    .locations
                    .get(pointer)
                    .map(|location| location.register)
                else {
                    return Ok(false);
                };
                if pointer != Eabi::FIRST_GENERAL_ARGUMENT {
                    return Ok(false);
                }
                let byte = pointer + 1;
                let accumulator = pointer + 2;
                self.load_integer_constant(accumulator, 0);
                let iteration = self.fresh_label();
                let test = self.fresh_label();
                self.emit_branch_to(test);
                self.bind_label(iteration);
                self.emit_ascii_hash_iteration(
                    byte,
                    accumulator,
                    Some(Instruction::AddImmediate {
                        d: pointer,
                        a: pointer,
                        immediate: 1,
                    }),
                    None,
                );
                self.bind_label(test);
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: byte,
                    a: pointer,
                    offset: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: byte,
                        immediate: 0,
                    });
                self.emit_branch_conditional_to(4, 2, iteration);
                self.output.instructions.push(Instruction::move_register(
                    Eabi::FIRST_GENERAL_ARGUMENT,
                    accumulator,
                ));
            }
            AsciiHashLoop::Bounded { pointer, bound } => {
                let Some(pointer) = self
                    .locations
                    .get(pointer)
                    .map(|location| location.register)
                else {
                    return Ok(false);
                };
                let Some(bound) = self.locations.get(bound).map(|location| location.register)
                else {
                    return Ok(false);
                };
                if pointer != Eabi::FIRST_GENERAL_ARGUMENT || bound != pointer + 1 {
                    return Ok(false);
                }
                let byte = bound + 1;
                let accumulator = byte + 1;
                let index = accumulator + 1;
                self.load_integer_constant(accumulator, 0);
                self.load_integer_constant(index, 0);
                let iteration = self.fresh_label();
                let test = self.fresh_label();
                let exit = self.fresh_label();
                self.emit_branch_to(test);
                self.bind_label(iteration);
                self.emit_ascii_hash_iteration(
                    byte,
                    accumulator,
                    Some(Instruction::AddImmediate {
                        d: index,
                        a: index,
                        immediate: 1,
                    }),
                    Some(Instruction::AddImmediate {
                        d: pointer,
                        a: pointer,
                        immediate: 1,
                    }),
                );
                self.bind_label(test);
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWord { a: index, b: bound });
                self.emit_branch_conditional_to(4, 0, exit); // bge
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: byte,
                    a: pointer,
                    offset: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: byte,
                        immediate: 0,
                    });
                self.emit_branch_conditional_to(4, 2, iteration);
                self.bind_label(exit);
                self.output.instructions.push(Instruction::move_register(
                    Eabi::FIRST_GENERAL_ARGUMENT,
                    accumulator,
                ));
            }
            AsciiHashLoop::PrefixSeeded { seed, pointer } => {
                let Some(seed) = self.locations.get(seed).map(|location| location.register) else {
                    return Ok(false);
                };
                let Some(pointer) = self
                    .locations
                    .get(pointer)
                    .map(|location| location.register)
                else {
                    return Ok(false);
                };
                if seed != Eabi::FIRST_GENERAL_ARGUMENT || pointer != seed + 1 {
                    return Ok(false);
                }
                let byte = pointer + 1;
                let iteration = self.fresh_label();
                let test = self.fresh_label();
                self.emit_branch_to(test);
                self.bind_label(iteration);
                self.emit_ascii_hash_iteration(
                    byte,
                    seed,
                    Some(Instruction::AddImmediate {
                        d: pointer,
                        a: pointer,
                        immediate: 1,
                    }),
                    None,
                );
                self.bind_label(test);
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: byte,
                    a: pointer,
                    offset: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: byte,
                        immediate: 0,
                    });
                self.emit_branch_conditional_to(4, 2, iteration);
            }
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
