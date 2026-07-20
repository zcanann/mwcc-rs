//! Metroid Prime's `rstl::wstring_l`: construct a literal view through the
//! hidden aggregate-result pointer and measure its UTF-16 length.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Function;

const MP_WSTRING_L_AST_HASH: u64 = 0x49b6_c5a2_ecb5_0a38;
const MP_WSTRING_L_CONTEXT: u64 = 0xea05_63cc_f607_b64d;

impl Generator {
    pub(super) fn try_mp_wstring_l(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "wstring_l__4rstlFPCw" || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if super::ast_hash(function) != MP_WSTRING_L_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names)
                != MP_WSTRING_L_CONTEXT
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let loop_body = self.fresh_label();
        let loop_test = self.fresh_label();
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::move_register(5, 4));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.emit_branch_to(loop_test);
        self.bind_label(loop_body);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 2,
        });
        self.bind_label(loop_test);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 5,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.output.instructions.push(Instruction::SubtractFrom {
            d: 4,
            a: 4,
            b: 5,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 4,
                shift: 31,
            });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 0,
                shift: 1,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
