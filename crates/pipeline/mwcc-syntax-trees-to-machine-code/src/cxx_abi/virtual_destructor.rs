//! Version- and optimization-specific virtual-destructor schedules.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::Function;
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention, Optimization};

/// The unoptimized predecrement ABI makes the hidden signed deleting flag a
/// real stack parameter. Keeping this schedule separate prevents the ordinary
/// optimized wrapper from accumulating O0 spill policy.
pub(super) fn lower_unoptimized_weak(
    function: &Function,
    behavior: &Behavior,
    config: &CompilerConfig,
    weak_vtable: bool,
    deleting_callee: &str,
) -> Option<MachineFunction> {
    if !weak_vtable
        || config.flags.inline_deferred
        || behavior.frame_convention != FrameConvention::Predecrement
        || behavior.optimization != Optimization::O0
    {
        return None;
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -32,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        },
        Instruction::move_register(31, 3),
        Instruction::StoreHalfword {
            s: 4,
            a: 1,
            offset: 8,
        },
        Instruction::CompareWordImmediate {
            a: 31,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 13,
        },
        Instruction::LoadHalfwordAlgebraic {
            d: 0,
            a: 1,
            offset: 8,
        },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 13,
        },
        Instruction::move_register(3, 31),
        Instruction::BranchAndLink {
            target: deleting_callee.to_owned(),
        },
        Instruction::move_register(3, 31),
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![Relocation {
        instruction_index: 12,
        kind: RelocationKind::Rel24,
        target: RelocationTarget::External(deleting_callee.to_owned()),
    }];
    output.symbol_order = vec![deleting_callee.to_owned()];
    output.referenced_function_symbols = vec![deleting_callee.to_owned()];
    output.implicit_external_callees = vec![deleting_callee.to_owned()];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    output.anonymous_label_bump = u32::from(behavior.cxx_virtual_destructor_label_bump);
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 1,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}
