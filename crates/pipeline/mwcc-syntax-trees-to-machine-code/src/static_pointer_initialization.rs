//! First-use initialization of C++ function-local pointer objects.
//!
//! The old frontend guards even constant pointer expressions. This owner keeps
//! the guard protocol and its versioned schedule separate from ordinary loads.
//! The initial entry point handles a getter whose only work is this declaration;
//! declarations inside general control flow need an explicit source-positioned
//! initialization operation before they can share this protocol.

use mwcc_machine_code::{
    Instruction as I, MachineFunction, Relocation, RelocationKind as K, RelocationTarget as T,
    StaticLocal,
};
use mwcc_syntax_trees::{
    Expression, Function, GlobalDeclaration, LocalDataRelocationTarget, Statement, Type,
};
use mwcc_versions::{
    CompilerConfig, GlobalAddressing, Optimization, StaticPointerInitializationStyle as Style,
};

pub(crate) fn lower_getter(
    function: &Function,
    globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Option<MachineFunction> {
    let style = config.build.profile.static_pointer_initialization_style();
    if style == Style::ConstantData
        || config.flags.global_addressing != GlobalAddressing::SmallData
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return None;
    }
    let returned = match (function.statements.as_slice(), &function.return_expression) {
        ([], Some(Expression::Variable(name))) => name,
        ([Statement::Return(Some(Expression::Variable(name)))], None) => name,
        _ => return None,
    };
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if !local.is_static
        || local.is_volatile
        || local.array_length.is_some()
        || local.name != *returned
        || local.name == "init"
        || globals
            .iter()
            .any(|global| global.name == local.name || global.name == "init")
        || !matches!(
            local.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }
    let mut output = MachineFunction::new(function.name.clone());
    let mut address_offset = 0;
    let (target, immediate, far) = match local.data_relocations.as_slice() {
        [relocation] if relocation.offset == 0 => {
            address_offset = i16::try_from(relocation.addend).ok()?;
            match &relocation.target {
                LocalDataRelocationTarget::StringLiteral(bytes) => {
                    output.string_literals.push(bytes.clone());
                    let small = !config.flags.string_literals_packed
                        && bytes.len() < 8
                        && (!config.flags.string_literals_read_only
                            || config.flags.read_only_global_addressing
                                == GlobalAddressing::SmallData);
                    (Some("@@str0".to_owned()), None, !small)
                }
                LocalDataRelocationTarget::Symbol(name) => {
                    let global = globals.iter().find(|global| global.name == *name)?;
                    if global.section.is_some() {
                        return None;
                    }
                    if global.is_const && global.array_length.is_none() {
                        output.keep_named_const_scalars.push(name.clone());
                    }
                    let size = match global.declared_type {
                        Type::Struct { size, .. } => size,
                        other => u32::from(other.width()) / 8,
                    } * global.array_length.map_or(1, u32::from);
                    let far = size > 8
                        || (global.is_const
                            && config.flags.read_only_global_addressing
                                != GlobalAddressing::SmallData);
                    (Some(name.clone()), None, far)
                }
            }
        }
        [] => {
            let word = match (&local.data_bytes, &local.initializer) {
                (Some(bytes), _) => i32::from_be_bytes(bytes.as_slice().try_into().ok()?),
                (None, Some(Expression::IntegerLiteral(0))) => 0,
                _ => return None,
            };
            // Even explicit null initialization uses the old frontend's guard.
            // Larger integer addresses need a separate construction schedule.
            let immediate = i16::try_from(word).ok()?;
            (None, Some(immediate), false)
        }
        _ => return None,
    };
    let unoptimized = config.flags.optimization == Optimization::O0;
    emit(
        &mut output,
        I::LoadByteZero {
            d: 0,
            a: 0,
            offset: 0,
        },
        K::EmbSda21,
        "init",
    );
    output.instructions.push(if unoptimized {
        I::ExtendSignByte { a: 0, s: 0 }
    } else {
        I::ExtendSignByteRecord { a: 0, s: 0 }
    });
    if unoptimized {
        output
            .instructions
            .push(I::CompareWordImmediate { a: 0, immediate: 0 });
    }
    let branch = output.instructions.len();
    output.instructions.push(I::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: 0,
    });
    let base_register = if unoptimized && address_offset == 0 {
        0
    } else {
        3
    };
    let value_register =
        if unoptimized || (address_offset != 0 && far && style == Style::GuardedScheduled) {
            0
        } else {
            3
        };
    if far {
        let target = target.as_deref()?;
        emit(
            &mut output,
            I::load_immediate_shifted(3, 0),
            K::Addr16Ha,
            target,
        );
        if !unoptimized && style == Style::GuardedScheduled {
            output.instructions.push(I::load_immediate(0, 1));
        }
        emit(
            &mut output,
            I::AddImmediate {
                d: base_register,
                a: 3,
                immediate: 0,
            },
            K::Addr16Lo,
            target,
        );
    } else if let Some(target) = &target {
        emit(
            &mut output,
            I::load_immediate(base_register, 0),
            K::EmbSda21,
            target,
        );
    } else {
        output
            .instructions
            .push(I::load_immediate(value_register, immediate?));
    }
    let delayed_offset = !unoptimized && far && style == Style::GuardedScheduled;
    if address_offset != 0 && !delayed_offset {
        output.instructions.push(I::AddImmediate {
            d: value_register,
            a: base_register,
            immediate: address_offset,
        });
    }
    if !unoptimized && !(far && style == Style::GuardedScheduled) {
        output.instructions.push(I::load_immediate(0, 1));
    }
    if !unoptimized && far && style == Style::GuardedScheduled {
        emit(
            &mut output,
            I::StoreByte {
                s: 0,
                a: 0,
                offset: 0,
            },
            K::EmbSda21,
            "init",
        );
        if address_offset != 0 {
            output.instructions.push(I::AddImmediate {
                d: value_register,
                a: base_register,
                immediate: address_offset,
            });
        }
        emit(
            &mut output,
            I::StoreWord {
                s: value_register,
                a: 0,
                offset: 0,
            },
            K::EmbSda21,
            &local.name,
        );
    } else {
        emit(
            &mut output,
            I::StoreWord {
                s: value_register,
                a: 0,
                offset: 0,
            },
            K::EmbSda21,
            &local.name,
        );
        if unoptimized {
            output.instructions.push(I::load_immediate(0, 1));
        }
        emit(
            &mut output,
            I::StoreByte {
                s: 0,
                a: 0,
                offset: 0,
            },
            K::EmbSda21,
            "init",
        );
    }
    let loaded = output.instructions.len();
    output.instructions[branch] = I::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: loaded,
    };
    emit(
        &mut output,
        I::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        },
        K::EmbSda21,
        &local.name,
    );
    output.instructions.push(I::BranchToLinkRegister);
    output.static_locals = vec![
        StaticLocal {
            name: local.name.clone(),
            initial_bytes: None,
            size: 4,
            alignment: 4.max(local.attribute_alignment.map_or(1, u32::from)),
            is_const: false,
            relocations: Vec::new(),
        },
        StaticLocal {
            name: "init".into(),
            initial_bytes: None,
            size: 1,
            alignment: 1,
            is_const: false,
            relocations: Vec::new(),
        },
    ];
    output.anonymous_label_bump = 1;
    output.is_static = function.is_static;
    output.static_locals_lead = true;
    output.is_weak = function.is_weak;
    output.text_deferred = function.text_deferred;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    Some(output)
}

fn emit(output: &mut MachineFunction, instruction: I, kind: K, target: &str) {
    output.relocations.push(Relocation {
        instruction_index: output.instructions.len(),
        kind,
        target: T::External(target.to_owned()),
    });
    output.instructions.push(instruction);
    if !output.symbol_order.iter().any(|name| name == target) {
        output.symbol_order.push(target.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Pointee};
    use mwcc_versions::{GC_2_6, GC_3_0A3};

    fn getter() -> Function {
        Function {
            return_type: Type::Pointer(Pointee::Int),
            name: "get".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Int),
                name: "p".into(),
                initializer: Some(Expression::IntegerLiteral(0)),
                is_volatile: false,
                array_length: None,
                is_static: true,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("p".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn explicit_null_uses_the_old_guard_but_no_initializer_does_not() {
        let config = CompilerConfig::new(GC_2_6);
        let mut function = getter();
        assert!(lower_getter(&function, &[], config).is_some());
        function.locals[0].initializer = None;
        assert!(lower_getter(&function, &[], config).is_none());
    }

    #[test]
    fn general_executable_statements_do_not_move_into_the_getter_protocol() {
        let mut function = getter();
        function
            .statements
            .push(Statement::Expression(Expression::Call {
                name: "observe".into(),
                arguments: Vec::new(),
            }));
        assert!(lower_getter(&function, &[], CompilerConfig::new(GC_2_6)).is_none());
    }

    #[test]
    fn constant_data_frontends_do_not_synthesize_first_use_work() {
        assert!(lower_getter(&getter(), &[], CompilerConfig::new(GC_3_0A3)).is_none());
    }
}
