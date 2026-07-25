//! Link-node constructor scheduling for the 1.2.5 generation.
//!
//! A small polymorphic list node installs two construction-phase vptrs, clears
//! its three links, and stores the caller's name. The early optimizer overlaps
//! the second vtable address with the first store; ordinary statement lowering
//! cannot recover that schedule after treating each store independently.

use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use mwcc_versions::CompilerConfig;

struct Shape {
    first_vtable: String,
    second_vtable: String,
    zero_offsets: [i16; 3],
    name_offset: i16,
}

pub(crate) fn lower(function: &Function, config: CompilerConfig) -> Option<MachineFunction> {
    if config.build.version != (2, 3, 3) || config.build.build != 163 {
        return None;
    }
    let shape = recognize(function)?;
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::load_immediate_shifted(5, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(5, 0),
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        },
        Instruction::load_immediate(0, 0),
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.zero_offsets[2],
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.zero_offsets[1],
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.zero_offsets[0],
        },
        Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: shape.name_offset,
        },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        relocation(0, RelocationKind::Addr16Ha, &shape.first_vtable),
        relocation(1, RelocationKind::Addr16Lo, &shape.first_vtable),
        relocation(2, RelocationKind::Addr16Ha, &shape.second_vtable),
        relocation(4, RelocationKind::Addr16Lo, &shape.second_vtable),
    ];
    output.symbol_order = vec![shape.first_vtable, shape.second_vtable];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    Some(output)
}

fn recognize(function: &Function) -> Option<Shape> {
    if !function.name.starts_with("__ct__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || function.parameters[1].name != "name"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || !matches!(function.parameters[1].parameter_type, Type::Pointer(_))
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let [first, second, zero_chain, name_store] = function.statements.as_slice() else {
        return None;
    };
    let first_vtable = vptr(first)?;
    let second_vtable = vptr(second)?;
    let zero_offsets = chained_zero_offsets(zero_chain)?;
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: name_offset,
                index_stride: None,
                ..
            },
        value: Expression::Variable(name),
    } = name_store
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(this) if this == "this") || name != "name" {
        return None;
    }
    Some(Shape {
        first_vtable,
        second_vtable,
        zero_offsets,
        name_offset: i16::try_from(*name_offset).ok()?,
    })
}

fn vptr(statement: &Statement) -> Option<String> {
    let (name, addend, offset) = super::parse_vptr_store(statement)?;
    (addend == 0 && offset == 0).then_some(name)
}

fn chained_zero_offsets(statement: &Statement) -> Option<[i16; 3]> {
    let Statement::Store {
        target: outer,
        value:
            Expression::Assign {
                target: middle,
                value: inner_assignment,
            },
    } = statement
    else {
        return None;
    };
    let Expression::Assign {
        target: inner,
        value: inner_value,
    } = inner_assignment.as_ref()
    else {
        return None;
    };
    if !matches!(inner_value.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    Some([
        member_offset(outer)?,
        member_offset(middle)?,
        member_offset(inner)?,
    ])
}

fn member_offset(expression: &Expression) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == "this")
        .then(|| i16::try_from(*offset).ok())
        .flatten()
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
