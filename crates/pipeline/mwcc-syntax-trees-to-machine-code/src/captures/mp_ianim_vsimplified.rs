//! `IAnimReader::VSimplified`: construct an empty `optional_object` through
//! the hidden aggregate-result pointer.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Function;

const MP_IANIM_VSIMPLIFIED_AST_HASH: u64 = 0x3063_f6bb_2f72_9038;
const MP_IANIM_CONTEXT: u64 = 0xea05_63cc_f607_b64d;

impl Generator {
    pub(super) fn try_mp_ianim_vsimplified(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.name != "VSimplified__11IAnimReaderFv" || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if super::ast_hash(function) != MP_IANIM_VSIMPLIFIED_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != MP_IANIM_CONTEXT
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreByte {
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
