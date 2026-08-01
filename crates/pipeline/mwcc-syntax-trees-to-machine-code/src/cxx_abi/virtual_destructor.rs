//! Version- and optimization-specific virtual-destructor schedules.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::Function;
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention, Optimization};

/// Lower the legacy linkage-first deleting destructor with one direct primary
/// base. The signed deleting flag lives in r31 and the complete object in r30;
/// keeping those roles explicit avoids asking general allocation to infer an
/// ABI-selected lifetime schedule from ordinary scalar liveness.
pub(super) fn lower_linkage_first_composed(
    function: &Function,
    own_vtable: String,
    own_offset: u32,
    base_callee: &str,
    deleting_callee: &str,
    cpp_exceptions: bool,
) -> Option<MachineFunction> {
    let own_offset = i16::try_from(own_offset).ok()?;
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -24,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 16,
        },
        Instruction::OrRecord { a: 30, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 18,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: own_offset,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: 0,
        },
        Instruction::load_immediate(4, 0),
        Instruction::BranchAndLink {
            target: base_callee.to_owned(),
        },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 31 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 18,
        },
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::BranchAndLink {
            target: deleting_callee.to_owned(),
        },
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 20,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 16,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        Relocation {
            instruction_index: 8,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(own_vtable.clone()),
        },
        Relocation {
            instruction_index: 9,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(own_vtable.clone()),
        },
        Relocation {
            instruction_index: 13,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(base_callee.to_owned()),
        },
        Relocation {
            instruction_index: 17,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(deleting_callee.to_owned()),
        },
    ];
    output.symbol_order = vec![
        own_vtable,
        base_callee.to_owned(),
        deleting_callee.to_owned(),
    ];
    output.referenced_function_symbols =
        vec![base_callee.to_owned(), deleting_callee.to_owned()];
    output.implicit_external_callees = output.referenced_function_symbols.clone();
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 2,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}

/// The unoptimized predecrement ABI makes the hidden signed deleting flag a
/// real stack parameter and omits the synthesized self-vptr restore. This is a
/// property of the O0 destructor schedule, independent of whether the class's
/// vtable is weak or owned by this translation unit.
pub(super) fn lower_unoptimized(
    function: &Function,
    behavior: &Behavior,
    config: &CompilerConfig,
    deleting_callee: &str,
) -> Option<MachineFunction> {
    if config.flags.inline_deferred
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Expression, Parameter, Type};
    use mwcc_versions::WII_1_0;

    #[test]
    fn linkage_first_composed_destructor_uses_flag_then_object_homes() {
        let function = Function {
            return_type: Type::StructPointer { element_size: 36 },
            name: "__dt__7ActRopeFv".into(),
            is_static: false,
            is_weak: true,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let output = lower_linkage_first_composed(
            &function,
            "__vt__7ActRope".into(),
            0,
            "__dt__6ActionFv",
            "__dl__FPv",
            false,
        )
        .unwrap();
        let actual = output
            .encode_text()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, "7c0802a6900100049421ffe893e100143be4000093c100107c7e1b794182002c3c60000038030000901e0000387e000038800000480000017fe007354081000c7fc3f378480000017fc3f3788001001c83e1001483c10010382100187c0803a64e800020");
    }

    #[test]
    fn unoptimized_owned_vtable_destructor_homes_the_deleting_flag() {
        let function = Function {
            return_type: Type::StructPointer { element_size: 16 },
            name: "__dt__10JAIAudibleFv".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 16 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Short,
                    name: "__destroy".into(),
                },
            ],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("this".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let mut config = CompilerConfig::new(WII_1_0);
        config.flags.optimization = Optimization::O0;
        config.flags.inline_enabled = false;
        let behavior = Behavior::resolve(&config);

        let output = lower_unoptimized(&function, &behavior, &config, "__dl__FPv").unwrap();
        let actual = output
            .encode_text()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(actual, "9421ffe07c0802a69001002493e1001c7c7f1b78b08100082c1f000041820018a80100082c0000004081000c7fe3fb78480000017fe3fb7883e1001c800100247c0803a6382100204e800020");
    }
}
