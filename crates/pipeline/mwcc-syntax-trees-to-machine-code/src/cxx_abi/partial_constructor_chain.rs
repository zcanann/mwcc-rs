//! Scheduling for a partially inlined 1.2.5 constructor chain.
//!
//! Two inline base layers may leave a deeper weak constructor call, followed
//! by construction-phase vptr installs, node initialization, and one inlined
//! scalar member setter. This schedule owns the link frame and keeps the
//! surviving base call ahead of the linkage save, as measured for build 163.

use std::collections::HashMap;

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use mwcc_versions::CompilerConfig;

struct Shape {
    base_constructor: String,
    base_name: Vec<u8>,
    node_vtable: String,
    node_initializer: String,
    intermediate_vtable: String,
    derived_vtable: String,
    derived_name: Vec<u8>,
    derived_initializer: String,
    payload_global: String,
    payload_offset: i16,
    payload_value: i16,
}

pub(crate) fn lower(
    function: &Function,
    source_inline_string_symbols: &HashMap<Vec<u8>, String>,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if config.build.version != (2, 3, 3) || config.build.build != 163 {
        return None;
    }
    let shape = recognize(function)?;
    let base_string = source_inline_string_symbols
        .get(&shape.base_name)
        .cloned()
        .unwrap_or_else(|| "@@str0".to_string());
    let derived_string = source_inline_string_symbols
        .get(&shape.derived_name)
        .cloned()
        .unwrap_or_else(|| "@@str1".to_string());

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::load_immediate(4, 0),
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
            a: 3,
            immediate: 0,
        },
        Instruction::BranchAndLink {
            target: shape.base_constructor.clone(),
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        },
        Instruction::load_immediate(4, 0),
        Instruction::BranchAndLink {
            target: shape.node_initializer.clone(),
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        },
        Instruction::BranchAndLink {
            target: shape.derived_initializer.clone(),
        },
        Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        },
        Instruction::load_immediate(0, shape.payload_value),
        Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: shape.payload_offset,
        },
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
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        relocation(1, RelocationKind::EmbSda21, &base_string),
        relocation(6, RelocationKind::Rel24, &shape.base_constructor),
        relocation(7, RelocationKind::Addr16Ha, &shape.node_vtable),
        relocation(8, RelocationKind::Addr16Lo, &shape.node_vtable),
        relocation(11, RelocationKind::EmbSda21, &base_string),
        relocation(12, RelocationKind::Rel24, &shape.node_initializer),
        relocation(13, RelocationKind::Addr16Ha, &shape.intermediate_vtable),
        relocation(14, RelocationKind::Addr16Lo, &shape.intermediate_vtable),
        relocation(15, RelocationKind::Addr16Ha, &shape.derived_vtable),
        relocation(17, RelocationKind::Addr16Lo, &shape.derived_vtable),
        relocation(18, RelocationKind::Addr16Ha, &derived_string),
        relocation(20, RelocationKind::Addr16Lo, &derived_string),
        relocation(22, RelocationKind::Rel24, &shape.derived_initializer),
        relocation(23, RelocationKind::EmbSda21, &shape.payload_global),
    ];
    output.symbol_order = vec![
        base_string.clone(),
        shape.base_constructor.clone(),
        shape.node_vtable,
        shape.node_initializer.clone(),
        shape.intermediate_vtable,
        shape.derived_vtable,
        derived_string.clone(),
        shape.derived_initializer.clone(),
        shape.payload_global,
    ];
    output.referenced_function_symbols = vec![
        shape.base_constructor,
        shape.node_initializer.clone(),
        shape.derived_initializer.clone(),
    ];
    output.implicit_external_callees =
        vec![shape.node_initializer, shape.derived_initializer];
    output.string_literals = vec![shape.base_name.clone(), shape.derived_name.clone()];
    if let Some(symbol) = source_inline_string_symbols.get(&shape.base_name) {
        output.string_literal_symbols.insert(0, symbol.clone());
    }
    if let Some(symbol) = source_inline_string_symbols.get(&shape.derived_name) {
        output.string_literal_symbols.insert(1, symbol.clone());
    }
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 1,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}

fn recognize(function: &Function) -> Option<Shape> {
    if !function.name.starts_with("__ct__")
        || function.parameters.len() != 1
        || function.parameters[0].name != "this"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || function.locals.len() != 1
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let [base, node_vptr, node_init, intermediate_vptr, derived_vptr, derived_init, payload] =
        function.statements.as_slice()
    else {
        return None;
    };
    let (base_constructor, base_name) = string_call(base)?;
    let node_vtable = vptr(node_vptr)?;
    let (node_initializer, repeated_base_name) = string_call(node_init)?;
    if repeated_base_name != base_name {
        return None;
    }
    let intermediate_vtable = vptr(intermediate_vptr)?;
    let derived_vtable = vptr(derived_vptr)?;
    let (derived_initializer, derived_name) = string_call(derived_init)?;
    let (payload_global, payload_offset, payload_value) =
        scalar_member_payload(payload, &function.locals[0].name)?;
    Some(Shape {
        base_constructor,
        base_name,
        node_vtable,
        node_initializer,
        intermediate_vtable,
        derived_vtable,
        derived_name,
        derived_initializer,
        payload_global,
        payload_offset,
        payload_value,
    })
}

fn string_call(statement: &Statement) -> Option<(String, Vec<u8>)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [Expression::Variable(this), Expression::StringLiteral(bytes)] = arguments.as_slice()
    else {
        return None;
    };
    (this == "this").then(|| (name.clone(), bytes.clone()))
}

fn vptr(statement: &Statement) -> Option<String> {
    let (name, addend, offset) = super::parse_vptr_store(statement)?;
    (addend == 0 && offset == 0).then_some(name)
}

fn scalar_member_payload(statement: &Statement, local: &str) -> Option<(String, i16, i16)> {
    let Statement::Expression(Expression::Comma { left, right }) = statement else {
        return None;
    };
    let Expression::Assign {
        target: first_target,
        value: first_value,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Variable(first_local) = first_target.as_ref() else {
        return None;
    };
    let Expression::Variable(global) = first_value.as_ref() else {
        return None;
    };
    if first_local != local {
        return None;
    }
    let Expression::Comma {
        left: store,
        right: terminal,
    } = right.as_ref()
    else {
        return None;
    };
    if !matches!(terminal.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let Expression::Assign {
        target,
        value: payload,
    } = store.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = target.as_ref()
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    let Expression::IntegerLiteral(value) = payload.as_ref() else {
        return None;
    };
    if base != local {
        return None;
    }
    Some((
        global.clone(),
        i16::try_from(*offset).ok()?,
        i16::try_from(*value).ok()?,
    ))
}

fn relocation(
    instruction_index: usize,
    kind: RelocationKind,
    target: &str,
) -> Relocation {
    Relocation {
        instruction_index,
        kind,
        target: RelocationTarget::External(target.to_string()),
    }
}

