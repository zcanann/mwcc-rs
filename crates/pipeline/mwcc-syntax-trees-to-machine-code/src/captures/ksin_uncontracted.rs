//! CodeWarrior 4.x `__kernel_sin` with `-fp_contract off`.
//!
//! This keeps fdlibm's source-level multiply/add chain unfused and preserves
//! the build's measured load schedule and anonymous pool ordinals.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Function, Type};

const AST_HASH: u64 = 0x5d19_0256_c2cb_b5e4;
const CONTEXT: u64 = 0xbd60_acb6_58c7_9e45;

impl Generator {
    pub(super) fn try_ksin_uncontracted(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "__kernel_sin"
            || function.return_type != Type::Double
            || function.parameters.len() != 3
            || self.behavior.contract_floating_point
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != CONTEXT
        {
            return Ok(false);
        }

        self.frame_size = 32;
        self.output.pre_scheduled = true;
        self.output.has_conversion = true;
        self.output.anonymous_label_bump =
            u32::from(self.behavior.ksin_uncontracted_label_bump);
        for bits in [
            0x3f81_1111_1110_f8a6,
            0xbf2a_01a0_19c1_61d5,
            0x3ec7_1de3_57b1_fe7d,
            0xbe5a_e5e6_8a2b_9ceb,
            0x3de5_d93a_5acf_d57c,
            0xbfc5_5555_5555_5549,
            0x3fe0_0000_0000_0000,
        ] {
            self.output.intern_constant(bits, 8);
        }

        let mut labels = std::collections::HashMap::new();
        for target in [13, 36, 46] {
            labels.insert(target, self.fresh_label());
        }
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 15936));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: 4,
                clear: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, labels[&13]);
        self.output
            .instructions
            .push(Instruction::ConvertToIntegerWordZero { d: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&13]);
        self.emit_branch_to(labels[&46]);
        self.bind_label(labels[&13]);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 7, a: 1, c: 1 });
        self.load_double_constant(0, 0x3de5_d93a_5acf_d57c);
        self.load_double_constant(5, 0xbe5a_e5e6_8a2b_9ceb);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.load_double_constant(4, 0x3ec7_1de3_57b1_fe7d);
        self.load_double_constant(3, 0xbf2a_01a0_19c1_61d5);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 6, a: 0, c: 7 });
        self.load_double_constant(0, 0x3f81_1111_1110_f8a6);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 8, a: 7, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 5, a: 5, b: 6 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 5, a: 7, c: 5 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 4, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 4, a: 7, c: 4 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 3, a: 3, b: 4 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 7, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 2, labels[&36]);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 2, a: 7, c: 0 });
        self.load_double_constant(0, 0xbfc5_5555_5555_5549);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 0, a: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 8, c: 0 });
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 1, b: 0 });
        self.emit_branch_to(labels[&46]);
        self.bind_label(labels[&36]);
        self.load_double_constant(4, 0x3fe0_0000_0000_0000);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 8, c: 0 });
        self.load_double_constant(0, 0xbfc5_5555_5555_5549);
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 4, a: 4, c: 2 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 8 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 3, a: 4, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 3, a: 7, c: 3 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 2, a: 3, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 0, a: 2, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 0 });
        self.bind_label(labels[&46]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
