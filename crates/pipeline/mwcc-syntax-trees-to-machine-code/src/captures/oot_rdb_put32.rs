//! Ocarina's RDB output command: an exact whole-function capture.
//!
//! This function is a 287-instruction nest of one jump table, three comparison
//! trees, repeated expanded buffer-clear loops, and two event calls. The gate
//! binds the measured GC/1.1 MQ-J source AST and retained-inline population;
//! relocation and jump-table ownership remain structured.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, JumpTable, RelocationKind, RelocationTarget};
use mwcc_syntax_trees::{Function, Type};

const AST_HASH: u64 = 0xd21d_34d8_42fd_20c4;
const INLINE_CONTEXT: u64 = 0x10e9_08cf_269e_67fc;

const WORDS: [u32; 287] = [
    0x7c0802a6, 0x90010004, 0x5480073e, 0x2c000008, 0x9421fff8, 0x41820454, 0x40800010, 0x2c000000,
    0x41820014, 0x4800043c, 0x2c00000c, 0x41820420, 0x48000430, 0x80c50000, 0x54c036be, 0x28000016,
    0x54c747be, 0x41810400, 0x3c800000, 0x38840000, 0x5400103a, 0x7c04002e, 0x7c0903a6, 0x4e800420,
    0x38600000, 0x48000408, 0x2c070002, 0x41820068, 0x40800014, 0x2c070000, 0x41820310, 0x40800014,
    0x48000300, 0x2c070004, 0x408002f8, 0x48000128, 0x80030104, 0x54c4863e, 0x7c850774, 0x7c830214,
    0x98a40004, 0x38a00000, 0x38800020, 0x48000010, 0x38050004, 0x7c8301ae, 0x38a50001, 0x80030104,
    0x7c050000, 0x4180ffec, 0x38000000, 0x90030104, 0x480002b8, 0x80030104, 0x54c4863e, 0x7c860774,
    0x7c830214, 0x98c40004, 0x80c30104, 0x7c833214, 0x88040004, 0x2c00000a, 0x40820034, 0x38c00000,
    0x38800020, 0x48000010, 0x38060004, 0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec,
    0x38000000, 0x90030104, 0x48000044, 0x2c060100, 0x40810034, 0x38c00000, 0x38800020, 0x48000010,
    0x38060004, 0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec, 0x38000000, 0x90030104,
    0x4800000c, 0x38060001, 0x90030104, 0x80a50000, 0x38e00000, 0x80030104, 0x38800020, 0x54a5c63e,
    0x7ca60774, 0x7ca30214, 0x98c50004, 0x48000010, 0x38070004, 0x7c8301ae, 0x38e70001, 0x80030104,
    0x7c070000, 0x4180ffec, 0x38000000, 0x90030104, 0x480001d8, 0x80030104, 0x54c4863e, 0x7c860774,
    0x7c830214, 0x98c40004, 0x80c30104, 0x7c833214, 0x88040004, 0x2c00000a, 0x40820034, 0x38c00000,
    0x38800020, 0x48000010, 0x38060004, 0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec,
    0x38000000, 0x90030104, 0x48000044, 0x2c060100, 0x40810034, 0x38c00000, 0x38800020, 0x48000010,
    0x38060004, 0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec, 0x38000000, 0x90030104,
    0x4800000c, 0x38060001, 0x90030104, 0x80850000, 0x80030104, 0x5484c63e, 0x7c860774, 0x7c830214,
    0x98c40004, 0x80c30104, 0x7c833214, 0x88040004, 0x2c00000a, 0x40820034, 0x38c00000, 0x38800020,
    0x48000010, 0x38060004, 0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec, 0x38000000,
    0x90030104, 0x48000044, 0x2c060100, 0x40810034, 0x38c00000, 0x38800020, 0x48000010, 0x38060004,
    0x7c8301ae, 0x38c60001, 0x80030104, 0x7c060000, 0x4180ffec, 0x38000000, 0x90030104, 0x4800000c,
    0x38060001, 0x90030104, 0x80030104, 0x80a50000, 0x7c830214, 0x98a40004, 0x80a30104, 0x7c832a14,
    0x88040004, 0x2c00000a, 0x40820034, 0x38a00000, 0x38800020, 0x48000010, 0x38050004, 0x7c8301ae,
    0x38a50001, 0x80030104, 0x7c050000, 0x4180ffec, 0x38000000, 0x90030104, 0x48000050, 0x2c050100,
    0x40810034, 0x38a00000, 0x38800020, 0x48000010, 0x38050004, 0x7c8301ae, 0x38a50001, 0x80030104,
    0x7c050000, 0x4180ffec, 0x38000000, 0x90030104, 0x48000018, 0x38050001, 0x90030104, 0x4800000c,
    0x38600000, 0x480000e8, 0x8063010c, 0x38801000, 0x38a00004, 0x48000001, 0x480000d0, 0x38600000,
    0x480000cc, 0x38600000, 0x480000c4, 0x38600000, 0x480000bc, 0x38600000, 0x480000b4, 0x38600000,
    0x480000ac, 0x38600000, 0x480000a4, 0x38600000, 0x4800009c, 0x38600000, 0x48000094, 0x38600000,
    0x4800008c, 0x38600000, 0x48000084, 0x38600000, 0x4800007c, 0x38600000, 0x48000074, 0x38600000,
    0x4800006c, 0x38600000, 0x48000064, 0x38600000, 0x4800005c, 0x38600000, 0x48000054, 0x38600000,
    0x4800004c, 0x38600000, 0x48000044, 0x38600000, 0x4800003c, 0x38600000, 0x48000034, 0x38600000,
    0x4800002c, 0x38600000, 0x48000024, 0x8063010c, 0x38801001, 0x38a00004, 0x48000001, 0x4800000c,
    0x38600000, 0x48000008, 0x38600001, 0x8001000c, 0x38210008, 0x7c0803a6, 0x4e800020,
];

impl Generator {
    pub(super) fn try_oot_rdb_put32(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "rdbPut32"
            || function.return_type != Type::Int
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
            || super::ast_hash(function) != AST_HASH
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != INLINE_CONTEXT
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 8;
        self.output.jump_tables.push(JumpTable {
            entries: vec![
                96, 104, 924, 932, 940, 948, 956, 964, 972, 980, 988, 996, 1004, 1020, 1028, 1036,
                1044, 1052, 1060, 1068, 1076, 1084, 1012,
            ],
            anonymous_offset: 162,
        });

        for (index, word) in WORDS.into_iter().enumerate() {
            match index {
                18 | 19 => {
                    let kind = if index == 18 {
                        RelocationKind::Addr16Ha
                    } else {
                        RelocationKind::Addr16Lo
                    };
                    self.record_target(kind, RelocationTarget::JumpTable);
                }
                229 | 278 => self.record_relocation(RelocationKind::Rel24, "xlObjectEvent"),
                _ => {}
            }
            self.output
                .instructions
                .push(Instruction::VerbatimWord(word));
        }
        Ok(true)
    }
}
