//! `IAnimReader::VGetAdvancementResults`: copy the requested animation time
//! and inline the default delta construction into the hidden result object.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::Function;

const MP_IANIM_ADVANCEMENT_AST_HASH: u64 = 0x4bfd_9ac0_85c0_03c1;
const MP_IANIM_CONTEXT: u64 = 0xea05_63cc_f607_b64d;

impl Generator {
    pub(super) fn try_mp_ianim_advancement(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.name
            != "VGetAdvancementResults__11IAnimReaderCFRC13CCharAnimTimeRC13CCharAnimTime"
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if super::ast_hash(function) != MP_IANIM_ADVANCEMENT_AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != MP_IANIM_CONTEXT
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 0,
                a: 5,
                offset: 0,
            });
        self.record_relocation(RelocationKind::Addr16Ha, "sZeroVector__9CVector3f");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(6, 0));
        self.record_relocation(RelocationKind::Addr16Ha, "sNoRotation__11CQuaternion");
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: 0,
                a: 3,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::LoadWord { d: 0, a: 5, offset: 4 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        });
        self.record_relocation(RelocationKind::Addr16Lo, "sZeroVector__9CVector3f");
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleWithUpdate {
                d: 0,
                a: 6,
                offset: 0,
            });
        for (source, destination) in [(0, 8), (4, 12), (8, 16)] {
            if source != 0 {
                self.output
                    .instructions
                    .push(Instruction::LoadFloatSingle {
                        d: 0,
                        a: 6,
                        offset: source,
                    });
            }
            self.output
                .instructions
                .push(Instruction::StoreFloatSingle {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
        }
        self.record_relocation(RelocationKind::Addr16Lo, "sNoRotation__11CQuaternion");
        self.output
            .instructions
            .push(Instruction::LoadFloatSingleWithUpdate {
                d: 0,
                a: 4,
                offset: 0,
            });
        for (source, destination) in [(0, 20), (4, 24), (8, 28), (12, 32)] {
            if source != 0 {
                self.output
                    .instructions
                    .push(Instruction::LoadFloatSingle {
                        d: 0,
                        a: 4,
                        offset: source,
                    });
            }
            self.output
                .instructions
                .push(Instruction::StoreFloatSingle {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
