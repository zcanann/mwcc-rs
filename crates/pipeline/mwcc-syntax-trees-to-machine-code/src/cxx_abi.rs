//! Lowering for compiler-generated CodeWarrior C++ ABI functions.
//!
//! These functions carry calling-convention state that does not exist in the
//! written source (notably a virtual destructor's deleting flag). Keeping their
//! recognition and fixed ABI skeleton here prevents that state from leaking
//! into the ordinary C control-flow and register-allocation owners.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};
use mwcc_versions::{Behavior, CompilerConfig};

/// Lower an empty polymorphic constructor whose only compiler-generated action
/// is installing the primary vptr. Constructors with member/base initialization
/// or a written body remain on the general lowering path.
pub(crate) fn lower_virtual_constructor(
    function: &Function,
    globals: &[GlobalDeclaration],
) -> Option<MachineFunction> {
    if !function.name.starts_with("__ct__")
        || function.parameters.len() != 1
        || function.parameters[0].name != "this"
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.statements.len() != 1
        || !matches!(
            function.return_expression.as_ref(),
            Some(mwcc_syntax_trees::Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let mwcc_syntax_trees::Statement::Store { target, value } = &function.statements[0] else {
        return None;
    };
    let mwcc_syntax_trees::Expression::Member { offset, .. } = target else {
        return None;
    };
    let vptr_offset = i16::try_from(*offset).ok()?;
    let mwcc_syntax_trees::Expression::AddressOf { operand } = value else {
        return None;
    };
    let mwcc_syntax_trees::Expression::Variable(vtable) = operand.as_ref() else {
        return None;
    };
    globals.iter().find(|global| global.name == *vtable)?;

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::load_immediate_shifted(4, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: vptr_offset,
        },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        Relocation {
            instruction_index: 0,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(vtable.clone()),
        },
        Relocation {
            instruction_index: 1,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(vtable.clone()),
        },
    ];
    output.symbol_order = vec![vtable.clone()];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    Some(output)
}

/// Lower the canonical virtual deleting-destructor shape synthesized by the
/// frontend. `None` means the function is ordinary source code and belongs to
/// general lowering.
pub(crate) fn lower_virtual_destructor(
    function: &Function,
    globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if !function.name.starts_with("__dt__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || function.parameters[1].name != "__destroy"
        || function.parameters[1].parameter_type != Type::Short
    {
        return None;
    }
    // The vtable relocation is the frontend's durable marker that this was an
    // ABI-synthesized *virtual* destructor, not a source function whose name
    // merely resembles one.
    let vtable = globals.iter().find(|global| {
        global.name.starts_with("__vt__")
            && global
                .data_relocations
                .iter()
                .any(|(offset, target, addend)| {
                    *offset == 8 && target == &function.name && *addend == 0
                })
    })?;

    let vptr_offset = function.statements.first().and_then(|statement| {
        let mwcc_syntax_trees::Statement::If { then_body, .. } = statement else {
            return None;
        };
        let mwcc_syntax_trees::Statement::Store { target, .. } = then_body.first()? else {
            return None;
        };
        let mwcc_syntax_trees::Expression::Member { offset, .. } = target else {
            return None;
        };
        i16::try_from(*offset).ok()
    })?;

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
        Instruction::OrRecord {
            a: 31,
            s: 3,
            b: 3,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 13,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 },
        Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: vptr_offset,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 13,
        },
        Instruction::Or {
            a: 3,
            s: 31,
            b: 31,
        },
        Instruction::BranchAndLink {
            target: "__dl__FPv".to_string(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::Or {
            a: 3,
            s: 31,
            b: 31,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 20,
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
            instruction_index: 6,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(vtable.name.clone()),
        },
        Relocation {
            instruction_index: 7,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(vtable.name.clone()),
        },
        Relocation {
            instruction_index: 12,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("__dl__FPv".to_string()),
        },
    ];
    output.symbol_order = vec![vtable.name.clone(), "__dl__FPv".to_string()];
    output.referenced_function_symbols = vec!["__dl__FPv".to_string()];
    output.implicit_external_callees = vec!["__dl__FPv".to_string()];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    output.anonymous_label_bump =
        u32::from(Behavior::resolve(&config).cxx_virtual_destructor_label_bump);
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 1,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}
