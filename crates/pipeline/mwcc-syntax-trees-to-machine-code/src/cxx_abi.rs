//! Lowering for compiler-generated CodeWarrior C++ ABI functions.
//!
//! These functions carry calling-convention state that does not exist in the
//! written source (notably a virtual destructor's deleting flag). Keeping their
//! recognition and fixed ABI skeleton here prevents that state from leaking
//! into the ordinary C control-flow and register-allocation owners.

mod adjustor_thunks;
mod virtual_destructor;

pub(crate) use adjustor_thunks::lower_vtable_adjustor_thunks;

use crate::InlineSummaries;
use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{
    AggregateDefinition, BinaryOperator, Expression, Function, GlobalDeclaration, Statement, Type,
};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention};

/// Lower a leaf function that returns one aggregate member by value through
/// the EABI hidden-result pointer. The source aggregate graph supplies the
/// scalar field classes erased from [`Type::Struct`], so a vector of floats is
/// copied with `lfs`/`stfs` rather than an unsafe raw-word approximation.
pub(crate) fn lower_aggregate_member_return(
    function: &Function,
    aggregate_definitions: &std::collections::HashMap<String, AggregateDefinition>,
    function_return_aggregate_tags: &std::collections::HashMap<String, String>,
) -> Option<MachineFunction> {
    let Type::Struct { size, .. } = function.return_type else {
        return None;
    };
    // Aggregates larger than eight bytes use r3 as a caller-provided result
    // address. Small aggregate register returns remain a separate ABI family.
    if size <= 8
        || function.parameters.len() != 1
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || !function.guards.is_empty()
    {
        return None;
    }
    let Some(Expression::Member {
        base,
        offset: source_offset,
        member_type,
        index_stride: None,
    }) = function.return_expression.as_ref()
    else {
        return None;
    };
    if *member_type != function.return_type
        || !matches!(base.as_ref(), Expression::Variable(name) if name == &function.parameters[0].name)
    {
        return None;
    }
    let aggregate_tag = function_return_aggregate_tags.get(&function.name)?;
    let definition = aggregate_definitions.get(aggregate_tag)?;
    if definition.byte_size != size {
        return None;
    }

    let mut output = MachineFunction::new(function.name.clone());
    emit_hidden_result_fields(
        &mut output.instructions,
        definition,
        aggregate_definitions,
        *source_offset,
        0,
    )?;
    output.instructions.push(Instruction::BranchToLinkRegister);
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    Some(output)
}

/// Copy one aggregate's declared fields from `source_offset(r4)` to
/// `destination_offset(r3)`. Padding is deliberately untouched: C++ value
/// return copies members, and MWCC likewise omits padding traffic.
fn emit_hidden_result_fields(
    instructions: &mut Vec<Instruction>,
    definition: &AggregateDefinition,
    aggregate_definitions: &std::collections::HashMap<String, AggregateDefinition>,
    source_offset: u32,
    destination_offset: u32,
) -> Option<()> {
    if definition.is_union {
        return None;
    }
    for member in &definition.members {
        if member.array_length.is_some() || member.bit_field.is_some() {
            return None;
        }
        let source = i16::try_from(source_offset.checked_add(member.offset)?).ok()?;
        let destination =
            i16::try_from(destination_offset.checked_add(member.offset)?).ok()?;
        match member.declared_type {
            Type::Float => {
                instructions.push(Instruction::LoadFloatSingle {
                    d: 0,
                    a: 4,
                    offset: source,
                });
                instructions.push(Instruction::StoreFloatSingle {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
            }
            Type::Double => {
                instructions.push(Instruction::LoadFloatDouble {
                    d: 0,
                    a: 4,
                    offset: source,
                });
                instructions.push(Instruction::StoreFloatDouble {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
            }
            Type::Char | Type::UnsignedChar => {
                instructions.push(Instruction::LoadByteZero {
                    d: 0,
                    a: 4,
                    offset: source,
                });
                instructions.push(Instruction::StoreByte {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
            }
            Type::Short | Type::UnsignedShort => {
                instructions.push(Instruction::LoadHalfwordZero {
                    d: 0,
                    a: 4,
                    offset: source,
                });
                instructions.push(Instruction::StoreHalfword {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
            }
            Type::Int
            | Type::UnsignedInt
            | Type::Pointer(_)
            | Type::StructPointer { .. } => {
                instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 4,
                    offset: source,
                });
                instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 3,
                    offset: destination,
                });
            }
            Type::LongLong | Type::UnsignedLongLong => {
                for word_offset in [0i16, 4] {
                    instructions.push(Instruction::LoadWord {
                        d: 0,
                        a: 4,
                        offset: source.checked_add(word_offset)?,
                    });
                    instructions.push(Instruction::StoreWord {
                        s: 0,
                        a: 3,
                        offset: destination.checked_add(word_offset)?,
                    });
                }
            }
            Type::Struct { .. } => {
                let nested = aggregate_definitions.get(member.aggregate_tag.as_ref()?)?;
                emit_hidden_result_fields(
                    instructions,
                    nested,
                    aggregate_definitions,
                    source_offset.checked_add(member.offset)?,
                    destination_offset.checked_add(member.offset)?,
                )?;
            }
            Type::Void => return None,
        }
    }
    Some(())
}

/// Lower a constructor composed from non-virtual base calls, a complete vtable
/// group installation, and call-valued member stores. These values all depend
/// on the incoming complete-object pointer across calls, so the ordinary leaf
/// expression paths cannot schedule them independently. The frontend's AST is
/// the ABI contract; no class or target-project names participate here.
pub(crate) fn lower_composed_constructor(
    function: &Function,
    _globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if Behavior::resolve(&config).frame_convention != FrameConvention::Predecrement
        || !function.name.starts_with("__ct__")
        || function.parameters.len() != 1
        || function.parameters[0].name != "this"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }

    let vptr_start = function
        .statements
        .iter()
        .position(|statement| parse_vptr_store(statement).is_some())?;
    let base_calls: Vec<(String, u32)> = function.statements[..vptr_start]
        .iter()
        .map(parse_adjusted_call_statement)
        .collect::<Option<_>>()?;
    if base_calls.is_empty() {
        return None;
    }
    let mut vptr_end = vptr_start;
    let mut vptrs = Vec::new();
    while let Some(vptr) = function.statements.get(vptr_end).and_then(parse_vptr_store) {
        vptrs.push(vptr);
        vptr_end += 1;
    }
    if vptrs.len() < 2 || vptrs.len() > 8 {
        return None;
    }
    let vtable = &vptrs[0].0;
    if vptrs.iter().any(|(name, _, _)| name != vtable) {
        return None;
    }
    let tail_statements = &function.statements[vptr_end..];
    let tail_actions = parse_constructor_tail_actions(tail_statements)?;
    if tail_actions.is_empty()
        || tail_actions[..tail_actions.len() - 1]
            .iter()
            .any(|action| matches!(action, ConstructorTail::BitOr { .. }))
    {
        return None;
    }
    let merged_bit_or = match tail_actions.as_slice() {
        [ConstructorTail::BitOr {
            target_offset,
            mask,
        }] => Some((*target_offset, *mask)),
        _ => None,
    };

    let mut output = MachineFunction::new(function.name.clone());
    let mut relocations = Vec::new();
    let mut referenced_functions = Vec::new();
    let mut symbol_order = Vec::new();
    output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        },
        Instruction::Or { a: 31, s: 3, b: 3 },
    ]);

    for (index, (callee, adjustment)) in base_calls.iter().enumerate() {
        if index != 0 || *adjustment != 0 {
            emit_adjusted_this(&mut output.instructions, *adjustment)?;
        }
        let instruction_index = output.instructions.len();
        output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        relocations.push(Relocation {
            instruction_index,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(callee.clone()),
        });
        referenced_functions.push(callee.clone());
        symbol_order.push(callee.clone());
    }

    if let Some((member_offset, mask)) = merged_bit_or {
        // GC 4.1 keeps the merged read/modify/write value in r0 while it builds
        // a four-component complete vtable group in r7..r4. The two source
        // setters become one load/OR/store, with independent address work
        // filling the load and `lis` latency slots.
        if vptrs.len() != 4 {
            return None;
        }
        output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: i16::try_from(member_offset).ok()?,
        });
        let vtable_register = 7;
        let vtable_hi = output.instructions.len();
        output
            .instructions
            .push(Instruction::load_immediate_shifted(vtable_register, 0));
        let vtable_lo = output.instructions.len();
        output.instructions.push(Instruction::AddImmediate {
            d: vtable_register,
            a: vtable_register,
            immediate: 0,
        });
        output
            .instructions
            .push(Instruction::Or { a: 3, s: 31, b: 31 });
        relocations.extend([
            Relocation {
                instruction_index: vtable_hi,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External(vtable.clone()),
            },
            Relocation {
                instruction_index: vtable_lo,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External(vtable.clone()),
            },
        ]);
        symbol_order.push(vtable.clone());

        let mut vptr_registers = vec![vtable_register];
        for (index, (_, addend, _)) in vptrs.iter().enumerate().skip(1) {
            let register = vtable_register - index as u8;
            output.instructions.push(Instruction::AddImmediate {
                d: register,
                a: vtable_register,
                immediate: i16::try_from(*addend).ok()?,
            });
            vptr_registers.push(register);
            if index == 1 {
                output.instructions.push(Instruction::OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: mask,
                });
            }
        }
        for (index, (_, _, object_offset)) in vptrs.iter().enumerate() {
            output.instructions.push(Instruction::StoreWord {
                s: vptr_registers[index],
                a: 31,
                offset: i16::try_from(*object_offset).ok()?,
            });
        }
        output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: i16::try_from(member_offset).ok()?,
        });
    } else {
        let vtable_register = u8::try_from(vptrs.len() + 2).ok()?;
        let vtable_hi = output.instructions.len();
        output
            .instructions
            .push(Instruction::load_immediate_shifted(vtable_register, 0));
        // MWCC fills the address-materialization latency with the first trailing
        // call's adjusted `this` value.
        emit_adjusted_this(&mut output.instructions, tail_actions[0].adjustment()?)?;
        let vtable_lo = output.instructions.len();
        output.instructions.push(Instruction::AddImmediate {
            d: vtable_register,
            a: vtable_register,
            immediate: 0,
        });
        relocations.extend([
            Relocation {
                instruction_index: vtable_hi,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External(vtable.clone()),
            },
            Relocation {
                instruction_index: vtable_lo,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External(vtable.clone()),
            },
        ]);
        symbol_order.push(vtable.clone());

        let mut vptr_registers = vec![vtable_register];
        for (index, (_, addend, _)) in vptrs.iter().enumerate().skip(1) {
            let register = if vtable_register - index as u8 == 3 {
                0
            } else {
                vtable_register - index as u8
            };
            let immediate = i16::try_from(*addend).ok()?;
            output.instructions.push(Instruction::AddImmediate {
                d: register,
                a: vtable_register,
                immediate,
            });
            vptr_registers.push(register);
            if index == 1 {
                output.instructions.push(Instruction::StoreWord {
                    s: vptr_registers[0],
                    a: 31,
                    offset: i16::try_from(vptrs[0].2).ok()?,
                });
            }
        }
        for (index, (_, _, object_offset)) in vptrs.iter().enumerate().skip(1) {
            output.instructions.push(Instruction::StoreWord {
                s: vptr_registers[index],
                a: 31,
                offset: i16::try_from(*object_offset).ok()?,
            });
        }

        for (index, action) in tail_actions.iter().enumerate() {
            if let ConstructorTail::BitOr {
                target_offset,
                mask,
            } = action
            {
                output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 31,
                    offset: i16::try_from(*target_offset).ok()?,
                });
                output
                    .instructions
                    .push(Instruction::Or { a: 3, s: 31, b: 31 });
                output.instructions.push(Instruction::OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: *mask,
                });
                output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 31,
                    offset: i16::try_from(*target_offset).ok()?,
                });
                continue;
            }
            if index != 0 {
                emit_adjusted_this(&mut output.instructions, action.adjustment()?)?;
            }
            let callee = action.callee();
            let instruction_index = output.instructions.len();
            output.instructions.push(Instruction::BranchAndLink {
                target: callee.to_string(),
            });
            relocations.push(Relocation {
                instruction_index,
                kind: RelocationKind::Rel24,
                target: RelocationTarget::External(callee.to_string()),
            });
            if let ConstructorTail::StoreCall { target_offset, .. } = action {
                output.instructions.push(Instruction::StoreWord {
                    s: 3,
                    a: 31,
                    offset: i16::try_from(*target_offset).ok()?,
                });
            }
            referenced_functions.push(callee.to_string());
            symbol_order.push(callee.to_string());
        }
        if !matches!(tail_actions.last(), Some(ConstructorTail::BitOr { .. })) {
            output
                .instructions
                .push(Instruction::Or { a: 3, s: 31, b: 31 });
        }
    }

    output.instructions.extend([
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        },
        Instruction::BranchToLinkRegister,
    ]);
    output.relocations = relocations;
    output.symbol_order = symbol_order;
    output.referenced_function_symbols = referenced_functions.clone();
    output.implicit_external_callees = referenced_functions;
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

fn emit_adjusted_this(instructions: &mut Vec<Instruction>, adjustment: u32) -> Option<()> {
    if adjustment == 0 {
        instructions.push(Instruction::Or { a: 3, s: 31, b: 31 });
    } else {
        instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: i16::try_from(adjustment).ok()?,
        });
    }
    Some(())
}

fn this_adjustment(expression: &Expression) -> Option<u32> {
    match expression {
        Expression::Variable(name) if name == "this" => Some(0),
        Expression::MemberAddress { base, offset, .. } if matches!(base.as_ref(), Expression::Variable(name) if name == "this") => {
            Some(*offset)
        }
        _ => None,
    }
}

fn parse_adjusted_call_statement(statement: &Statement) -> Option<(String, u32)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [object] = arguments.as_slice() else {
        return None;
    };
    Some((name.clone(), this_adjustment(object)?))
}

fn parse_vptr_store(statement: &Statement) -> Option<(String, u32, u32)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let Expression::Member { base, offset, .. } = target else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == "this") {
        return None;
    }
    match value {
        Expression::AddressOf { operand } => {
            let Expression::Variable(vtable) = operand.as_ref() else {
                return None;
            };
            Some((vtable.clone(), 0, *offset))
        }
        Expression::MemberAddress {
            base,
            offset: addend,
            ..
        } => {
            let Expression::AddressOf { operand } = base.as_ref() else {
                return None;
            };
            let Expression::Variable(vtable) = operand.as_ref() else {
                return None;
            };
            Some((vtable.clone(), *addend, *offset))
        }
        _ => None,
    }
}

fn parse_call_valued_member_store(statement: &Statement) -> Option<ConstructorTail> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let Expression::Member { base, offset, .. } = target else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == "this") {
        return None;
    }
    let Expression::Call { name, arguments } = value else {
        return None;
    };
    let [object] = arguments.as_slice() else {
        return None;
    };
    Some(ConstructorTail::StoreCall {
        target_offset: *offset,
        callee: name.clone(),
        adjustment: this_adjustment(object)?,
    })
}

/// Parse the measured constructor-tail subset and merge adjacent bit setters
/// on the same complete-object member. Keeping calls and updates as typed
/// actions lets scheduling depend on semantics rather than callee names.
fn parse_constructor_tail_actions(statements: &[Statement]) -> Option<Vec<ConstructorTail>> {
    let mut actions = Vec::new();
    for statement in statements {
        if let Some(action) = parse_call_valued_member_store(statement) {
            actions.push(action);
            continue;
        }
        if let Some((callee, adjustment)) = parse_adjusted_call_statement(statement) {
            actions.push(ConstructorTail::Call(callee, adjustment));
            continue;
        }
        let (target_offset, mask) = parse_member_bit_or(statement)?;
        if let Some(ConstructorTail::BitOr {
            target_offset: previous_offset,
            mask: previous_mask,
        }) = actions.last_mut()
        {
            if *previous_offset != target_offset {
                return None;
            }
            *previous_mask |= mask;
        } else {
            actions.push(ConstructorTail::BitOr {
                target_offset,
                mask,
            });
        }
    }
    Some(actions)
}

fn parse_member_bit_or(statement: &Statement) -> Option<(u32, u16)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let offset = complete_object_member_offset(target)?;
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    if complete_object_member_offset(left)? != offset {
        return None;
    }
    let Expression::IntegerLiteral(value) = right.as_ref() else {
        return None;
    };
    let immediate = u16::try_from(*value).ok()?;
    (immediate != 0).then_some((offset, immediate))
}

fn complete_object_member_offset(expression: &Expression) -> Option<u32> {
    let Expression::Member { base, offset, .. } = expression else {
        return None;
    };
    this_adjustment(base)?.checked_add(*offset)
}

enum ConstructorTail {
    Call(String, u32),
    StoreCall {
        target_offset: u32,
        callee: String,
        adjustment: u32,
    },
    BitOr {
        target_offset: u32,
        mask: u16,
    },
}

impl ConstructorTail {
    fn callee(&self) -> &str {
        match self {
            Self::Call(callee, _) | Self::StoreCall { callee, .. } => callee,
            Self::BitOr { .. } => unreachable!("a bit update has no callee"),
        }
    }

    fn adjustment(&self) -> Option<u32> {
        match self {
            Self::Call(_, adjustment) | Self::StoreCall { adjustment, .. } => Some(*adjustment),
            Self::BitOr { .. } => None,
        }
    }
}

/// Lower a complete-object deleting destructor with explicit reverse-order
/// base calls. The frontend supplies the C++ lifetime semantics; this owner
/// supplies the two saved homes and the measured predecrement-frame schedule.
pub(crate) fn lower_composed_destructor(
    function: &Function,
    inline_summaries: &InlineSummaries,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if Behavior::resolve(&config).frame_convention != FrameConvention::Predecrement
        || !function.name.starts_with("__dt__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || function.parameters[1].name != "__destroy"
        || function.parameters[1].parameter_type != Type::Short
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(condition),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if condition != "this" || !else_body.is_empty() {
        return None;
    }
    let (delete_guard, lifetime_statements) = then_body.split_last()?;
    let (own_vptr, base_statements) = lifetime_statements
        .split_first()
        .and_then(|(first, rest)| parse_vptr_store(first).map(|store| (Some(store), rest)))
        .unwrap_or((None, lifetime_statements));
    let base_calls: Vec<(String, u32)> = base_statements
        .iter()
        .map(parse_base_destructor_call)
        .collect::<Option<_>>()?;
    if base_calls.is_empty() {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::Greater,
                left,
                right,
            },
        then_body: delete_body,
        else_body: delete_else,
    } = delete_guard
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        || !matches!(right.as_ref(), Expression::IntegerLiteral(0))
        || !delete_else.is_empty()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call {
        name: delete_callee,
        arguments: delete_arguments,
    })] = delete_body.as_slice()
    else {
        return None;
    };
    if !matches!(delete_arguments.as_slice(), [Expression::Variable(name)] if name == "this") {
        return None;
    }

    if Behavior::resolve(&config).frame_convention == FrameConvention::Predecrement
        && base_calls.len() == 1
        && base_calls[0].1 == 0
    {
        if let (Some((own_vtable, 0, own_offset)), Some(base)) = (
            own_vptr,
            inline_summaries.trivial_virtual_destructor(&base_calls[0].0),
        ) {
            return lower_inlined_trivial_base_destructor(
                function,
                own_vtable,
                own_offset,
                base.vtable.clone(),
                base.vptr_offset,
                delete_callee.clone(),
                config,
            );
        }
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        },
        Instruction::Or { a: 31, s: 4, b: 4 },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        },
        Instruction::Or { a: 30, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 0,
        },
    ]);
    let null_branch = 8;
    let mut relocations = Vec::new();
    let mut referenced_functions = Vec::new();
    for (index, (callee, adjustment)) in base_calls.iter().enumerate() {
        let inlined_base = (config.flags.ipa_file && base_calls.len() == 1 && *adjustment == 0)
            .then(|| inline_summaries.single_base_destructor(callee))
            .flatten()
            .filter(|summary| summary.adjustment == 0);
        if let Some(summary) = inlined_base {
            let skip_branch = output.instructions.len();
            output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 0,
                });
            output.instructions.push(Instruction::load_immediate(4, 0));
            let instruction_index = output.instructions.len();
            output.instructions.push(Instruction::BranchAndLink {
                target: summary.callee.clone(),
            });
            relocations.push(Relocation {
                instruction_index,
                kind: RelocationKind::Rel24,
                target: RelocationTarget::External(summary.callee.clone()),
            });
            referenced_functions.push(summary.callee.clone());
            let after_call = output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut output.instructions[skip_branch]
            {
                *target = after_call;
            }
            continue;
        }
        if index == 0 {
            output.instructions.push(Instruction::load_immediate(4, 0));
            if *adjustment != 0 {
                output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: i16::try_from(*adjustment).ok()?,
                });
            }
        } else {
            emit_adjusted_saved_object(&mut output.instructions, *adjustment)?;
            output.instructions.push(Instruction::load_immediate(4, 0));
        }
        let instruction_index = output.instructions.len();
        output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        relocations.push(Relocation {
            instruction_index,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(callee.clone()),
        });
        referenced_functions.push(callee.clone());
    }
    output.instructions.extend([
        Instruction::CompareWordImmediate {
            a: 31,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 0,
        },
        Instruction::Or { a: 3, s: 30, b: 30 },
    ]);
    let deleting_branch = output.instructions.len() - 2;
    let delete_call = output.instructions.len();
    output.instructions.push(Instruction::BranchAndLink {
        target: delete_callee.clone(),
    });
    relocations.push(Relocation {
        instruction_index: delete_call,
        kind: RelocationKind::Rel24,
        target: RelocationTarget::External(delete_callee.clone()),
    });
    referenced_functions.push(delete_callee.clone());
    let epilogue = output.instructions.len();
    if let Instruction::BranchConditionalForward { target, .. } =
        &mut output.instructions[null_branch]
    {
        *target = epilogue;
    }
    if let Instruction::BranchConditionalForward { target, .. } =
        &mut output.instructions[deleting_branch]
    {
        *target = epilogue;
    }
    output.instructions.extend([
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 8,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        },
        Instruction::BranchToLinkRegister,
    ]);
    output.relocations = relocations;
    output.symbol_order = referenced_functions.clone();
    output.referenced_function_symbols = referenced_functions.clone();
    output.implicit_external_callees = referenced_functions;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 2,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}

fn lower_inlined_trivial_base_destructor(
    function: &Function,
    own_vtable: String,
    own_offset: u32,
    base_vtable: String,
    base_offset: u32,
    delete_callee: String,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    let own_offset = i16::try_from(own_offset).ok()?;
    let base_offset = i16::try_from(base_offset).ok()?;
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 20 },
        Instruction::StoreWord { s: 31, a: 1, offset: 12 },
        Instruction::OrRecord { a: 31, s: 3, b: 3 },
        Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 17 },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
        Instruction::StoreWord { s: 0, a: 31, offset: own_offset },
        Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 13 },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
        Instruction::StoreWord { s: 0, a: 31, offset: base_offset },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 1, target: 17 },
        Instruction::Or { a: 3, s: 31, b: 31 },
        Instruction::BranchAndLink { target: delete_callee.clone() },
        Instruction::LoadWord { d: 0, a: 1, offset: 20 },
        Instruction::Or { a: 3, s: 31, b: 31 },
        Instruction::LoadWord { d: 31, a: 1, offset: 12 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        Relocation {
            instruction_index: 6,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(own_vtable.clone()),
        },
        Relocation {
            instruction_index: 7,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(own_vtable.clone()),
        },
        Relocation {
            instruction_index: 10,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(base_vtable.clone()),
        },
        Relocation {
            instruction_index: 11,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(base_vtable.clone()),
        },
        Relocation {
            instruction_index: 16,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(delete_callee.clone()),
        },
    ];
    output.symbol_order = vec![own_vtable, base_vtable, delete_callee.clone()];
    output.referenced_function_symbols = vec![delete_callee.clone()];
    output.implicit_external_callees = vec![delete_callee];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    let behavior = Behavior::resolve(&config);
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

fn emit_adjusted_saved_object(instructions: &mut Vec<Instruction>, adjustment: u32) -> Option<()> {
    if adjustment == 0 {
        instructions.push(Instruction::Or { a: 3, s: 30, b: 30 });
    } else {
        instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: i16::try_from(adjustment).ok()?,
        });
    }
    Some(())
}

fn parse_base_destructor_call(statement: &Statement) -> Option<(String, u32)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [object, Expression::IntegerLiteral(0)] = arguments.as_slice() else {
        return None;
    };
    Some((name.clone(), this_adjustment(object)?))
}

/// Lower an empty polymorphic constructor whose only compiler-generated action
/// is installing the primary vptr. Constructors with member/base initialization
/// or a written body remain on the general lowering path.
pub(crate) fn lower_virtual_constructor(
    function: &Function,
    globals: &[GlobalDeclaration],
    config: CompilerConfig,
) -> Option<MachineFunction> {
    if !function.name.starts_with("__ct__")
        || function.parameters.len() != 1
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

    let scalar_stores = function.statements[1..]
        .iter()
        .map(|statement| {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type: Type::Int,
                        ..
                    },
                value: Expression::IntegerLiteral(value),
            } = statement
            else {
                return None;
            };
            if !matches!(base.as_ref(), Expression::Variable(name) if name == "this") {
                return None;
            }
            Some((i16::try_from(*offset).ok()?, i16::try_from(*value).ok()?))
        })
        .collect::<Option<Vec<_>>>()?;

    let mut output = MachineFunction::new(function.name.clone());
    output
        .instructions
        .push(Instruction::load_immediate_shifted(4, 0));
    let value_register = if scalar_stores.is_empty() { 0 } else { 4 };
    output.instructions.push(Instruction::AddImmediate {
        d: value_register,
        a: 4,
        immediate: 0,
    });
    output.instructions.push(Instruction::StoreWord {
        s: value_register,
        a: 3,
        offset: vptr_offset,
    });
    for (offset, value) in scalar_stores {
        output
            .instructions
            .push(Instruction::load_immediate(0, value));
        output
            .instructions
            .push(Instruction::StoreWord { s: 0, a: 3, offset });
    }
    output.instructions.push(Instruction::BranchToLinkRegister);
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
    if config.build.version.0 >= 4
        && config.flags.debug_info
        && !function.statements[1..].is_empty()
    {
        // Fragmented class debug consumes the leaf constructor's ordinary
        // post-function analysis block before the following unwind pair.
        output.post_function_anonymous_bump = Some(0);
    }
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
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
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

    let (vptr_offset, deleting_callee) = function.statements.first().and_then(|statement| {
        let mwcc_syntax_trees::Statement::If { then_body, .. } = statement else {
            return None;
        };
        let mwcc_syntax_trees::Statement::Store { target, .. } = then_body.first()? else {
            return None;
        };
        let mwcc_syntax_trees::Expression::Member { offset, .. } = target else {
            return None;
        };
        let mwcc_syntax_trees::Statement::If {
            then_body: delete_body,
            ..
        } = then_body.get(1)?
        else {
            return None;
        };
        let [mwcc_syntax_trees::Statement::Expression(
            mwcc_syntax_trees::Expression::Call { name, arguments },
        )] = delete_body.as_slice()
        else {
            return None;
        };
        if !matches!(arguments.as_slice(), [mwcc_syntax_trees::Expression::Variable(name)] if name == "this") {
            return None;
        }
        Some((i16::try_from(*offset).ok()?, name.clone()))
    })?;

    let behavior = Behavior::resolve(&config);
    if let Some(output) = virtual_destructor::lower_unoptimized_weak(
        function,
        &behavior,
        &config,
        vtable.is_weak,
        &deleting_callee,
    ) {
        return Some(output);
    }
    let mut output = MachineFunction::new(function.name.clone());
    if vtable.is_weak
        && !config.flags.inline_deferred
        && behavior.frame_convention == FrameConvention::Predecrement
    {
        output.instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 10,
            },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 10,
            },
            Instruction::BranchAndLink {
                target: deleting_callee.clone(),
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations = vec![Relocation {
            instruction_index: 9,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(deleting_callee.clone()),
        }];
        output.symbol_order = vec![deleting_callee.clone()];
        output.referenced_function_symbols = vec![deleting_callee.clone()];
        output.implicit_external_callees = vec![deleting_callee];
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
        return Some(output);
    }
    output.instructions = if behavior.frame_convention == FrameConvention::Predecrement {
        vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::OrRecord { a: 31, s: 3, b: 3 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 12,
            },
            Instruction::load_immediate_shifted(5, 0),
            Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 },
            Instruction::AddImmediate {
                d: 0,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: vptr_offset,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 12,
            },
            Instruction::BranchAndLink {
                target: deleting_callee.clone(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]
    } else {
        vec![
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
            Instruction::OrRecord { a: 31, s: 3, b: 3 },
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
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::BranchAndLink {
                target: deleting_callee.clone(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::Or { a: 3, s: 31, b: 31 },
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
        ]
    };
    let (vtable_hi, vtable_lo, delete_call) =
        if behavior.frame_convention == FrameConvention::Predecrement {
            (6, 8, 11)
        } else {
            (6, 7, 12)
        };
    output.relocations = vec![
        Relocation {
            instruction_index: vtable_hi,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::External(vtable.name.clone()),
        },
        Relocation {
            instruction_index: vtable_lo,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External(vtable.name.clone()),
        },
        Relocation {
            instruction_index: delete_call,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(deleting_callee.clone()),
        },
    ];
    output.symbol_order = vec![vtable.name.clone(), deleting_callee.clone()];
    output.referenced_function_symbols = vec![deleting_callee.clone()];
    output.implicit_external_callees = vec![deleting_callee];
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
    use mwcc_syntax_trees::AggregateMember;

    #[test]
    fn copies_float_aggregate_fields_through_hidden_result_registers() {
        let definition = AggregateDefinition {
            source_tag: Some("Vec".to_string()),
            name: "Vec".to_string(),
            byte_size: 12,
            alignment: 4,
            is_union: false,
            members: ["x", "y", "z"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| AggregateMember {
                    name: name.to_string(),
                    declared_type: Type::Float,
                    source_fundamental: None,
                    offset: index as u32 * 4,
                    aggregate_tag: None,
                    array_length: None,
                    bit_field: None,
                })
                .collect(),
        };
        let mut instructions = Vec::new();
        emit_hidden_result_fields(
            &mut instructions,
            &definition,
            &std::collections::HashMap::new(),
            396,
            0,
        )
        .expect("flat float aggregate is supported");

        assert_eq!(
            instructions,
            vec![
                Instruction::LoadFloatSingle { d: 0, a: 4, offset: 396 },
                Instruction::StoreFloatSingle { s: 0, a: 3, offset: 0 },
                Instruction::LoadFloatSingle { d: 0, a: 4, offset: 400 },
                Instruction::StoreFloatSingle { s: 0, a: 3, offset: 4 },
                Instruction::LoadFloatSingle { d: 0, a: 4, offset: 404 },
                Instruction::StoreFloatSingle { s: 0, a: 3, offset: 8 },
            ]
        );
    }
}
