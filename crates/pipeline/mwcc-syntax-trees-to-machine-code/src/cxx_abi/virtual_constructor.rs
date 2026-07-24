//! Leaf polymorphic constructors.
//!
//! The compiler-generated vptr installation and simple written initialization
//! form one fixed-register transaction. Keeping that transaction here avoids
//! making the general expression allocator infer constructor ABI registers.

use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, Statement, Type};
use mwcc_versions::CompilerConfig;

enum TailAction {
    StoreImmediate { offset: i16, value: i16 },
    StoreThisGlobal { name: String },
}

/// Lower a polymorphic leaf constructor consisting of its synthesized primary
/// vptr store followed by word-sized constant member stores or assignments of
/// `this` to pointer globals. Calls, control flow, and computed values remain on
/// the general constructor path.
pub(crate) fn lower(
    function: &Function,
    globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if !function.name.starts_with("__ct__")
        || function.parameters.is_empty()
        || function.parameters[0].name != "this"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.statements.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }

    let (mut vtable, vptr_offset) = parse_vptr_store(&function.statements[0], globals)?;
    let mut tail_start = 1;
    if config.flags.ipa_file {
        while let Some((next_vtable, next_offset)) = function
            .statements
            .get(tail_start)
            .and_then(|statement| parse_vptr_store(statement, globals))
        {
            // An inlined trivial base constructor can install its vptr
            // immediately before the complete-object constructor overwrites
            // the same slot. File IPA retains only the final installation.
            if next_offset != vptr_offset {
                return None;
            }
            vtable = next_vtable;
            tail_start += 1;
        }
    }
    let actions = function.statements[tail_start..]
        .iter()
        .map(|statement| parse_tail_action(statement, globals))
        .collect::<Option<Vec<_>>>()?;

    let mut output = MachineFunction::new(function.name.clone());
    if config.flags.ipa_file {
        if let Some(value) = reused_immediate(&actions, vptr_offset) {
            emit_ipa_reused_immediate(
                &mut output,
                &vtable,
                vptr_offset,
                value,
                &actions,
            );
            finish(&mut output, function, &config);
            return Some(output);
        }
    }
    output
        .instructions
        .push(Instruction::load_immediate_shifted(4, 0));
    output.instructions.push(Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: 0,
    });
    output.instructions.push(Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: vptr_offset,
    });
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
    output.symbol_order = vec![vtable];

    for action in actions {
        match action {
            TailAction::StoreImmediate { offset, value } => {
                output
                    .instructions
                    .push(Instruction::load_immediate(0, value));
                output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset,
                });
            }
            TailAction::StoreThisGlobal { name } => {
                let instruction_index = output.instructions.len();
                output.instructions.push(Instruction::StoreWord {
                    s: 3,
                    a: 0,
                    offset: 0,
                });
                output.relocations.push(Relocation {
                    instruction_index,
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::External(name.clone()),
                });
                output.symbol_order.push(name);
            }
        }
    }

    output.instructions.push(Instruction::BranchToLinkRegister);
    finish(&mut output, function, &config);
    Some(output)
}

fn reused_immediate(actions: &[TailAction], vptr_offset: i16) -> Option<i16> {
    if actions.len() < 2 {
        return None;
    }
    let (first_offset, value) = match actions.first()? {
        TailAction::StoreImmediate { offset, value } => (*offset, *value),
        TailAction::StoreThisGlobal { .. } => return None,
    };
    if first_offset == vptr_offset {
        return None;
    }
    actions.iter().skip(1).all(|action| {
        matches!(
            action,
            TailAction::StoreImmediate {
                offset,
                value: found,
            } if *offset != vptr_offset && *found == value
        )
    })
    .then_some(value)
}

fn emit_ipa_reused_immediate(
    output: &mut MachineFunction,
    vtable: &str,
    vptr_offset: i16,
    value: i16,
    actions: &[TailAction],
) {
    output.instructions.extend([
        Instruction::load_immediate_shifted(4, 0),
        Instruction::load_immediate(0, value),
        Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        },
    ]);
    let first_offset = match &actions[0] {
        TailAction::StoreImmediate { offset, .. } => *offset,
        TailAction::StoreThisGlobal { .. } => {
            unreachable!("the reused-immediate schedule was validated")
        }
    };
    output.instructions.extend([
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: first_offset,
        },
        Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: vptr_offset,
        },
    ]);
    for action in &actions[1..] {
        let TailAction::StoreImmediate { offset, .. } = action else {
            unreachable!("the reused-immediate schedule was validated")
        };
        output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: *offset,
        });
    }
    output.instructions.push(Instruction::BranchToLinkRegister);
    output.relocations = vec![
        Relocation {
            instruction_index: 0,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(vtable.to_string()),
        },
        Relocation {
            instruction_index: 2,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(vtable.to_string()),
        },
    ];
    output.symbol_order = vec![vtable.to_string()];
}

fn finish(output: &mut MachineFunction, function: &Function, config: &CompilerConfig) {
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if config.build.version.0 >= 4 && config.flags.debug_info && function.statements.len() > 1 {
        // Fragmented class debug consumes the leaf constructor's ordinary
        // post-function analysis block before the following unwind pair.
        output.post_function_anonymous_bump = Some(0);
    }
}

fn parse_vptr_store(
    statement: &Statement,
    globals: &[GlobalDeclaration],
) -> Option<(String, i16)> {
    let Statement::Store {
        target: Expression::Member { offset, .. },
        value: Expression::AddressOf { operand },
    } = statement
    else {
        return None;
    };
    let Expression::Variable(vtable) = operand.as_ref() else {
        return None;
    };
    globals.iter().find(|global| global.name == *vtable)?;
    Some((vtable.clone(), i16::try_from(*offset).ok()?))
}

fn parse_tail_action(
    statement: &Statement,
    globals: &[GlobalDeclaration],
) -> Option<TailAction> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    match (target, value) {
        (
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            },
            Expression::IntegerLiteral(value),
        ) if matches!(base.as_ref(), Expression::Variable(name) if name == "this")
            && is_word_type(*member_type) =>
        {
            Some(TailAction::StoreImmediate {
                offset: i16::try_from(*offset).ok()?,
                value: i16::try_from(*value).ok()?,
            })
        }
        (Expression::Variable(name), Expression::Variable(value)) if value == "this" => {
            let global = globals.iter().find(|global| global.name == *name)?;
            is_pointer_type(global.declared_type).then(|| TailAction::StoreThisGlobal {
                name: name.clone(),
            })
        }
        _ => None,
    }
}

fn is_word_type(value_type: Type) -> bool {
    matches!(
        value_type,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

fn is_pointer_type(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}
