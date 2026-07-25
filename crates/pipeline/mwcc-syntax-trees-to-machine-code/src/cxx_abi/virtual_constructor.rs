//! Leaf polymorphic constructors.
//!
//! The compiler-generated vptr installation and simple written initialization
//! form one fixed-register transaction. Keeping that transaction here avoids
//! making the general expression allocator infer constructor ABI registers.

use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, Statement, Type};
use mwcc_versions::{CompilerConfig, Optimization};

enum TailAction {
    StoreImmediate { offset: i16, value: i16 },
    StoreThisGlobal { name: String },
}

struct ParameterizedDerivedConstructor {
    base_vtable: String,
    derived_vtable: String,
    vptr_offset: i16,
    member_offset: i16,
    parameter_register: u8,
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

    if config.build.version.0 < 4
        && config.flags.optimization == Optimization::O4
        && config.flags.scheduler_enabled
    {
        if let Some(shape) = recognize_parameterized_derived_constructor(function, globals) {
            let mut output = emit_parameterized_derived_constructor(function, shape);
            finish(&mut output, function, &config);
            return Some(output);
        }
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

/// GC 1.3 through 2.7 schedules the two construction-phase vtable addresses as
/// one O4 transaction. The base high half fills the derived high half's issue
/// slot, and the incoming member value remains live in its EABI parameter
/// register until both vptr stores complete.
fn recognize_parameterized_derived_constructor(
    function: &Function,
    globals: &[GlobalDeclaration],
) -> Option<ParameterizedDerivedConstructor> {
    let [base_store, derived_store, member_store] = function.statements.as_slice() else {
        return None;
    };
    let (base_vtable, vptr_offset) = parse_vptr_store(base_store, globals)?;
    let (derived_vtable, derived_offset) = parse_vptr_store(derived_store, globals)?;
    if base_vtable == derived_vtable || vptr_offset != derived_offset {
        return None;
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            },
        value: Expression::Variable(parameter),
    } = member_store
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == "this")
        || !is_word_type(*member_type)
        || *offset == u32::try_from(vptr_offset).ok()?
    {
        return None;
    }
    let parameter_index = function
        .parameters
        .iter()
        .position(|candidate| candidate.name == *parameter)?;
    if parameter_index == 0
        || parameter_index > 7
        || !is_word_type(function.parameters[parameter_index].parameter_type)
    {
        return None;
    }
    Some(ParameterizedDerivedConstructor {
        base_vtable,
        derived_vtable,
        vptr_offset,
        member_offset: i16::try_from(*offset).ok()?,
        parameter_register: 3 + u8::try_from(parameter_index).ok()?,
    })
}

fn emit_parameterized_derived_constructor(
    function: &Function,
    shape: ParameterizedDerivedConstructor,
) -> MachineFunction {
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::load_immediate_shifted(6, 0),
        Instruction::load_immediate_shifted(5, 0),
        Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 6,
            a: 3,
            offset: shape.vptr_offset,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.vptr_offset,
        },
        Instruction::StoreWord {
            s: shape.parameter_register,
            a: 3,
            offset: shape.member_offset,
        },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        Relocation {
            instruction_index: 0,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(shape.base_vtable.clone()),
        },
        Relocation {
            instruction_index: 1,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(shape.derived_vtable.clone()),
        },
        Relocation {
            instruction_index: 2,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(shape.base_vtable.clone()),
        },
        Relocation {
            instruction_index: 4,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(shape.derived_vtable.clone()),
        },
    ];
    output.symbol_order = vec![shape.base_vtable, shape.derived_vtable];
    output
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
