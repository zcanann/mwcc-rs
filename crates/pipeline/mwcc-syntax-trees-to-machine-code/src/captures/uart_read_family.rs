//! Shared emission for the MSL UART console-read family.
//!
//! Several SDK snapshots carry the same read-until-CR loop with different
//! inlining and frame conventions.  Keep the measured schedules here so the
//! small capture modules only own identity/context selection.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, RelocationKind};

pub(super) enum UartReadInitialization<'a> {
    Inline {
        initialized: &'a str,
        initialize: &'a str,
    },
    Call(&'a str),
}

pub(super) enum UartReadConvention {
    Predecrement,
    LinkageFirst,
}

pub(super) enum UartReadBoolean {
    SignBit,
    BranchAndNarrow,
}

impl Generator {
    /// Linkage-first `__write_console`: two pointer parameters survive an
    /// inlined one-time UART initializer, then feed the write call.
    pub(super) fn emit_linkage_first_uart_write(
        &mut self,
        initialized: &str,
        initialize: &str,
        writer: &str,
        anonymous_label_bump: u32,
    ) {
        self.non_leaf = true;
        self.frame_size = 40;
        let after_initialization = self.fresh_label();
        let initialized_ok = self.fresh_label();
        let write_ok = self.fresh_label();
        let exit = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 32,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 4,
            immediate: 0,
        });

        self.record_relocation(RelocationKind::EmbSda21, initialized);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 0,
            immediate: 0,
        });
        self.emit_branch_conditional_to(4, 2, after_initialization);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 1));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -7936,
        });
        self.record_relocation(RelocationKind::Rel24, initialize);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: initialize.to_string(),
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: 0,
        });
        self.emit_branch_conditional_to(4, 2, after_initialization);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.record_relocation(RelocationKind::EmbSda21, initialized);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });

        self.bind_label(after_initialization);
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 2, initialized_ok);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(exit);

        self.bind_label(initialized_ok);
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Rel24, writer);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: writer.to_string(),
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 2, write_ok);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(exit);
        self.bind_label(write_ok);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(exit);
        for (register, offset) in [(0, 44), (31, 36), (30, 32)] {
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: 1,
                offset,
            });
        }
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 40,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += anonymous_label_bump;
    }

    pub(super) fn emit_uart_read_family(
        &mut self,
        initialization: UartReadInitialization<'_>,
        convention: UartReadConvention,
        boolean: UartReadBoolean,
        reader: &str,
        anonymous_label_bump: u32,
    ) {
        self.non_leaf = true;
        self.frame_size = match convention {
            UartReadConvention::Predecrement => 32,
            UartReadConvention::LinkageFirst => 48,
        };

        let after_initialization = self.fresh_label();
        let initialized_ok = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_test = self.fresh_label();
        let loop_exit = self.fresh_label();
        let return_exit = self.fresh_label();

        match convention {
            UartReadConvention::Predecrement => {
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -32,
                    });
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                if matches!(initialization, UartReadInitialization::Inline { .. }) {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, 0));
                }
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 36,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 28,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 24,
                });
                self.output
                    .instructions
                    .push(Instruction::move_register(30, 5));
                self.output.instructions.push(Instruction::StoreWord {
                    s: 29,
                    a: 1,
                    offset: 20,
                });
                self.output
                    .instructions
                    .push(Instruction::move_register(29, 4));
            }
            UartReadConvention::LinkageFirst => {
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, 0));
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                });
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -48,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 44,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 40,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 30,
                    a: 5,
                    immediate: 0,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 29,
                    a: 1,
                    offset: 36,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 29,
                    a: 4,
                    immediate: 0,
                });
            }
        }

        match initialization {
            UartReadInitialization::Inline {
                initialized,
                initialize,
            } => {
                self.record_relocation(RelocationKind::EmbSda21, initialized);
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                });
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: 0,
                    immediate: 0,
                });
                self.emit_branch_conditional_to(4, 2, after_initialization);
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(3, 1));
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: -7936,
                });
                self.record_relocation(RelocationKind::Rel24, initialize);
                self.output.instructions.push(Instruction::BranchAndLink {
                    target: initialize.to_string(),
                });
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: 3,
                    immediate: 0,
                });
                self.emit_branch_conditional_to(4, 2, after_initialization);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 1));
                self.record_relocation(RelocationKind::EmbSda21, initialized);
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                });
                self.bind_label(after_initialization);
            }
            UartReadInitialization::Call(initializer) => {
                self.record_relocation(RelocationKind::Rel24, initializer);
                self.output.instructions.push(Instruction::BranchAndLink {
                    target: initializer.to_string(),
                });
            }
        }

        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 2, initialized_ok);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(return_exit);

        self.bind_label(initialized_ok);
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.emit_branch_to(loop_test);

        self.bind_label(loop_body);
        match convention {
            UartReadConvention::Predecrement => self
                .output
                .instructions
                .push(Instruction::move_register(3, 29)),
            UartReadConvention::LinkageFirst => {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 29,
                    immediate: 0,
                })
            }
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 1));
        self.record_relocation(RelocationKind::Rel24, reader);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: reader.to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 29,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 13,
            });
        self.emit_branch_conditional_to(12, 2, loop_exit);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 1,
        });

        self.bind_label(loop_test);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 31 });
        self.emit_branch_conditional_to(12, 1, loop_exit);
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: 0,
        });
        self.emit_branch_conditional_to(12, 2, loop_body);

        self.bind_label(loop_exit);
        match boolean {
            UartReadBoolean::SignBit => {
                self.output
                    .instructions
                    .push(Instruction::Negate { d: 0, a: 3 });
                self.output
                    .instructions
                    .push(Instruction::Or { a: 0, s: 0, b: 3 });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: 3,
                        s: 0,
                        shift: 31,
                    });
            }
            UartReadBoolean::BranchAndNarrow => {
                let nonzero = self.fresh_label();
                let narrowed = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: 3,
                    immediate: 0,
                });
                self.emit_branch_conditional_to(4, 2, nonzero);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 0));
                self.emit_branch_to(narrowed);
                self.bind_label(nonzero);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 1));
                self.bind_label(narrowed);
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: 3,
                        s: 0,
                        clear: 24,
                    });
            }
        }

        self.bind_label(return_exit);
        match convention {
            UartReadConvention::Predecrement => {
                for (register, offset) in [(0, 36), (31, 28), (30, 24), (29, 20)] {
                    self.output.instructions.push(Instruction::LoadWord {
                        d: register,
                        a: 1,
                        offset,
                    });
                }
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 32,
                });
            }
            UartReadConvention::LinkageFirst => {
                for (register, offset) in [(0, 52), (31, 44), (30, 40)] {
                    self.output.instructions.push(Instruction::LoadWord {
                        d: register,
                        a: 1,
                        offset,
                    });
                }
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
                self.output.instructions.push(Instruction::LoadWord {
                    d: 29,
                    a: 1,
                    offset: 36,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 48,
                });
            }
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += anonymous_label_bump;
    }
}
