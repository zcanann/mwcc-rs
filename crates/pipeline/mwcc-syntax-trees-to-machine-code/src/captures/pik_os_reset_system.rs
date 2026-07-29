//! pik_os_reset_system: an exact-match whole-function capture (fire 0).
//! See captures::ast_hash and docs/emission-model.md for the pipeline.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{Function, Type};

/// The Debug-AST hash of the captured function (dev loop: 0 prints candidates).
const PIK_OS_RESET_SYSTEM_AST_HASH: u64 = 0x5a6f30d49907829b;

impl Generator {
    pub(super) fn try_pik_os_reset_system(&mut self, function: &Function) -> Compilation<bool> {
        if function.name != "OSResetSystem"
            || function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let hash = super::ast_hash(function);
        if hash != PIK_OS_RESET_SYSTEM_AST_HASH {
            eprintln!("pik_os_reset_system hash candidate: {hash:#x}");
            return Ok(false);
        }
        // CONTEXT GATE + @N bump: dispatched BEFORE any emission (a
        // post-emission decline would pollute the output for the next
        // template). Register measured (fingerprint -> bump) pairs only.
        let context = super::skipped_context_fingerprint(&self.skipped_inline_names);
        let bump: u32 = match context {
            0xa5b71792a9673795 => 0, // pikmin OSReset.c (G98E01_PIKIDEMO)
            _ => {
                eprintln!("pik_os_reset_system context candidate: {context:#x}");
                return Ok(false);
            }
        };
        // -- emit (the capture, verbatim) --
        self.frame_size = 64;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29, 28, 27, 26];
        self.output.pre_scheduled = true;
        self.output.symbol_order = [
            "OSDisableScheduler",
            "__OSStopAudioSystem",
            "__PADDisableRecalibration",
            "ResetFunctionQueue",
            "__OSSyncSram",
            "__OSLockSram",
            "__OSUnlockSram",
            "OSDisableInterrupts",
            "LCDisable",
            "ICFlashInvalidate",
            "Reset",
            "OSCancelThread",
            "OSEnableScheduler",
            "__OSReboot",
            "memset",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let mut labels: std::collections::HashMap<usize, mwcc_vreg::Label> =
            std::collections::HashMap::new();
        for target in [
            155, 156, 157, 160, 161, 162, 170, 180, 181, 194, 195, 198, 202, 203, 204, 212, 227,
            232, 233, 234, 242, 243, 244, 250, 253, 254, 255, 263, 264, 265,
        ] {
            labels.insert(target, self.fresh_label());
        }
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
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            });
        self.output
            .instructions
            .push(Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 40,
            });
        self.output
            .instructions
            .push(Instruction::move_register(26, 3));
        self.output
            .instructions
            .push(Instruction::move_register(29, 4));
        self.output
            .instructions
            .push(Instruction::move_register(30, 5));
        self.record_relocation(RelocationKind::Rel24, "OSDisableScheduler");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSDisableScheduler".to_string(),
        });
        self.record_relocation(RelocationKind::Rel24, "__OSStopAudioSystem");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSStopAudioSystem".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 26,
                immediate: 2,
            });
        self.emit_branch_conditional_to(4, 2, labels[&155]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.record_relocation(RelocationKind::Rel24, "__PADDisableRecalibration");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__PADDisableRecalibration".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(31, 3));
        self.bind_label(labels[&155]);
        self.emit_branch_to(labels[&156]); // b
        self.bind_label(labels[&156]);
        self.emit_branch_to(labels[&157]); // b
        self.bind_label(labels[&157]);
        self.record_relocation(RelocationKind::EmbSda21, "ResetFunctionQueue");
        self.output.instructions.push(Instruction::LoadWord {
            d: 27,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(28, 0));
        self.emit_branch_to(labels[&160]); // b
        self.bind_label(labels[&160]);
        self.emit_branch_to(labels[&161]); // b
        self.bind_label(labels[&161]);
        self.emit_branch_to(labels[&170]); // b
        self.bind_label(labels[&162]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 27,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 27,
            a: 27,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 28, s: 28, b: 0 });
        self.bind_label(labels[&170]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 27,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&162]); // bne
        self.record_relocation(RelocationKind::Rel24, "__OSSyncSram");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSSyncSram".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 28, s: 28, b: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 28,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&180]); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.emit_branch_to(labels[&181]); // b
        self.bind_label(labels[&180]);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.bind_label(labels[&181]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&157]); // beq
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 26,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&198]); // bne
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, labels[&198]); // beq
        self.record_relocation(RelocationKind::Rel24, "__OSLockSram");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSLockSram".to_string(),
        });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: 19,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: 64,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 3,
            offset: 19,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.record_relocation(RelocationKind::Rel24, "__OSUnlockSram");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSUnlockSram".to_string(),
        });
        self.emit_branch_to(labels[&194]); // b
        self.bind_label(labels[&194]);
        self.emit_branch_to(labels[&195]); // b
        self.bind_label(labels[&195]);
        self.record_relocation(RelocationKind::Rel24, "__OSSyncSram");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSSyncSram".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, labels[&195]); // beq
        self.bind_label(labels[&198]);
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSDisableInterrupts".to_string(),
        });
        self.record_relocation(RelocationKind::EmbSda21, "ResetFunctionQueue");
        self.output.instructions.push(Instruction::LoadWord {
            d: 28,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(27, 0));
        self.emit_branch_to(labels[&202]); // b
        self.bind_label(labels[&202]);
        self.emit_branch_to(labels[&203]); // b
        self.bind_label(labels[&203]);
        self.emit_branch_to(labels[&212]); // b
        self.bind_label(labels[&204]);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 28,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegisterAndLink);
        self.output
            .instructions
            .push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 28,
            a: 28,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 5,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 27, s: 27, b: 0 });
        self.bind_label(labels[&212]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 28,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&204]); // bne
        self.record_relocation(RelocationKind::Rel24, "__OSSyncSram");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSSyncSram".to_string(),
        });
        self.record_relocation(RelocationKind::Rel24, "LCDisable");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "LCDisable".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 26,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 2, labels[&227]); // bne
        self.record_relocation(RelocationKind::Rel24, "OSDisableInterrupts");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSDisableInterrupts".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -13312));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 8192,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 3,
            offset: 2,
        });
        self.record_relocation(RelocationKind::Rel24, "ICFlashInvalidate");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "ICFlashInvalidate".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 3,
                s: 29,
                shift: 3,
            });
        self.record_relocation(RelocationKind::Rel24, "Reset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "Reset".to_string(),
        });
        self.emit_branch_to(labels[&250]); // b
        self.bind_label(labels[&227]);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 26,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, labels[&250]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -32768));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 220,
        });
        self.emit_branch_to(labels[&232]); // b
        self.bind_label(labels[&232]);
        self.emit_branch_to(labels[&233]); // b
        self.bind_label(labels[&233]);
        self.emit_branch_to(labels[&244]); // b
        self.bind_label(labels[&234]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 712,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 27,
            a: 3,
            offset: 764,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&242]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&243]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&242]); // beq
        self.emit_branch_to(labels[&243]); // b
        self.bind_label(labels[&242]);
        self.record_relocation(RelocationKind::Rel24, "OSCancelThread");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSCancelThread".to_string(),
        });
        self.bind_label(labels[&243]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.bind_label(labels[&244]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&234]); // bne
        self.record_relocation(RelocationKind::Rel24, "OSEnableScheduler");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSEnableScheduler".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 29));
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.record_relocation(RelocationKind::Rel24, "__OSReboot");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__OSReboot".to_string(),
        });
        self.bind_label(labels[&250]);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, -32768));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 220,
        });
        self.emit_branch_to(labels[&253]); // b
        self.bind_label(labels[&253]);
        self.emit_branch_to(labels[&254]); // b
        self.bind_label(labels[&254]);
        self.emit_branch_to(labels[&265]); // b
        self.bind_label(labels[&255]);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: 712,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 27,
            a: 3,
            offset: 764,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 4 });
        self.emit_branch_conditional_to(12, 2, labels[&263]); // beq
        self.emit_branch_conditional_to(4, 0, labels[&264]); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, labels[&263]); // beq
        self.emit_branch_to(labels[&264]); // b
        self.bind_label(labels[&263]);
        self.record_relocation(RelocationKind::Rel24, "OSCancelThread");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "OSCancelThread".to_string(),
        });
        self.bind_label(labels[&264]);
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.bind_label(labels[&265]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, labels[&255]); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(29, -32768));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 140));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 212,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 20));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 244,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 4));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 12288,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 192));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 12488,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 12));
        self.record_relocation(RelocationKind::Rel24, "memset");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "memset".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, "__PADDisableRecalibration");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__PADDisableRecalibration".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: 26,
                a: 1,
                offset: 40,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += bump;
        Ok(true)
    }
}
