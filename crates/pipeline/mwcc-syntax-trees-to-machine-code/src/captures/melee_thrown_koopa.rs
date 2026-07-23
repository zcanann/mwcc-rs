//! Melee's two thrown-Koopa state entries after inlining `GET_FIGHTER`.
//!
//! GC 1.2.5n schedules the inlined member chase with the surrounding state
//! setup and retains the inline's zero-initialized local as an unreferenced
//! `.sdata2` double. Keep both related schedules and their pool accounting in
//! one context-gated capture family.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

const CONTEXT: u64 = 0xa35e_ac30_65bf_4722;
const THROWN_KOOPA_AST_HASH: u64 = 0xe81c_b7dd_dbe5_6f1f;
const THROWN_KOOPA_ICE_AST_HASH: u64 = 0x0e07_b954_a5de_d413;

impl Generator {
    pub(super) fn try_melee_thrown_koopa(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.parameters.len() != 2
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != CONTEXT
        {
            return Ok(false);
        }

        let hash = super::ast_hash(function);
        match (function.name.as_str(), hash) {
            ("ftCo_800BCDE0", THROWN_KOOPA_AST_HASH) => self.emit_thrown_koopa(),
            ("ftCo_800BCE64", THROWN_KOOPA_ICE_AST_HASH) => self.emit_thrown_koopa_ice(),
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn prepare_thrown_koopa_frame(&mut self, saved_gprs: &[u8]) {
        self.frame_size = 48;
        self.non_leaf = true;
        self.output.pre_scheduled = true;
        self.callee_saved = saved_gprs.to_vec();
    }

    fn emit_thrown_koopa(&mut self) {
        self.prepare_thrown_koopa_frame(&[31, 30]);

        // The skipped inline's zeroed eight-byte local is retained at @191,
        // one slot before this body's ordinary pool position. Seven hidden
        // inline ordinals then separate it from the live float constants.
        self.output.constant_number_adjust = -1;
        self.output.intern_constant_image_new(0, 8);
        self.output.constant_number_gaps.push((1, 7));

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
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
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 3,
            offset: 44,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 31,
            offset: 6744,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 5,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 6,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 31,
                offset: 44,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 9024,
        });
        self.load_float_constant(1, 0.0);
        self.load_float_constant(2, 1.0);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 3, b: 1 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 31,
            offset: 6744,
        });
        self.call("Fighter_ChangeMotionState");
        self.store_callback_address();
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 511));
        self.call("ftCommon_8007E2F4");
        self.output
            .instructions
            .push(Instruction::move_register(3, 30));
        self.call("ftAnim_8006EBA4");
        self.emit_thrown_koopa_epilogue(&[31, 30]);
    }

    fn emit_thrown_koopa_ice(&mut self) {
        self.prepare_thrown_koopa_frame(&[31]);
        self.output.intern_constant_image_new(0, 8);

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(5, 3148));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 20608,
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 3,
            offset: 44,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 31,
            offset: 6744,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: 44,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: 6,
            offset: 44,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 31,
                offset: 44,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 9024,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 31,
            offset: 2196,
        });
        self.load_float_constant(2, 1.0);
        self.load_float_constant(3, 0.0);
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 31,
            offset: 6744,
        });
        self.call("Fighter_ChangeMotionState");
        self.store_callback_address();
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 511));
        self.call("ftCommon_8007E2F4");
        self.emit_thrown_koopa_epilogue(&[31]);
    }

    fn call(&mut self, target: &str) {
        self.record_relocation(RelocationKind::Rel24, target);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: target.to_string(),
        });
    }

    fn store_callback_address(&mut self) {
        self.record_relocation(RelocationKind::Addr16Ha, "ftCo_800DE508");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, "ftCo_800DE508");
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 8624,
        });
    }

    fn emit_thrown_koopa_epilogue(&mut self, saved_gprs: &[u8]) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 52,
        });
        for (&register, offset) in saved_gprs.iter().zip([44, 40]) {
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: 1,
                offset,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 48,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }
}
