//! Virtual destructors that destroy an embedded array of class objects.
//!
//! The frontend expresses the lifetime operation through `__destroy_arr`.
//! This owner keeps the legacy LinkageFirst register homes and deliberately
//! overlapped address-materialization schedule out of ordinary call lowering.

use crate::InlineSummaries;
use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention, Optimization};

struct ArrayMemberDestructor {
    own_vtable: String,
    member_offset: i16,
    element_destructor: String,
    element_size: i16,
    element_count: i16,
    base_vtable: String,
    delete_callee: String,
}

/// Lower the optimized legacy schedule for a virtual complete-object
/// destructor whose only non-base lifetime action is one embedded class array.
pub(crate) fn lower(
    function: &Function,
    inline_summaries: &InlineSummaries,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    let behavior = Behavior::resolve(&config);
    if behavior.frame_convention != FrameConvention::LinkageFirst
        || behavior.optimization != Optimization::O4
        || config.flags.cpp_exceptions
    {
        return None;
    }
    let shape = recognize(function, inline_summaries)?;
    Some(emit(function, shape))
}

fn emit(function: &Function, shape: ArrayMemberDestructor) -> MachineFunction {
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
        Instruction::OrRecord {
            a: 30,
            s: 3,
            b: 3,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 26,
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
            a: 30,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 3,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: shape.member_offset,
        },
        Instruction::load_immediate(5, shape.element_size),
        Instruction::load_immediate(6, shape.element_count),
        Instruction::BranchAndLink {
            target: "__destroy_arr".into(),
        },
        Instruction::CompareLogicalWordImmediate {
            a: 30,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 22,
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
            offset: 0,
        },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 31 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 26,
        },
        Instruction::move_register(3, 30),
        Instruction::BranchAndLink {
            target: shape.delete_callee.clone(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::move_register(3, 30),
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
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        relocation(8, RelocationKind::Addr16Ha, &shape.own_vtable),
        relocation(9, RelocationKind::Addr16Lo, &shape.own_vtable),
        relocation(10, RelocationKind::Addr16Ha, &shape.element_destructor),
        relocation(12, RelocationKind::Addr16Lo, &shape.element_destructor),
        relocation(16, RelocationKind::Rel24, "__destroy_arr"),
        relocation(19, RelocationKind::Addr16Ha, &shape.base_vtable),
        relocation(20, RelocationKind::Addr16Lo, &shape.base_vtable),
        relocation(25, RelocationKind::Rel24, &shape.delete_callee),
    ];
    output.symbol_order = vec![
        shape.own_vtable,
        shape.element_destructor.clone(),
        "__destroy_arr".into(),
        shape.base_vtable,
        shape.delete_callee.clone(),
    ];
    output.referenced_function_symbols = vec![
        shape.element_destructor,
        "__destroy_arr".into(),
        shape.delete_callee.clone(),
    ];
    output.implicit_external_callees = vec!["__destroy_arr".into(), shape.delete_callee];
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    output
}

fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
    Relocation {
        instruction_index,
        kind,
        target: RelocationTarget::External(target.into()),
    }
}

fn recognize(
    function: &Function,
    inline_summaries: &InlineSummaries,
) -> Option<ArrayMemberDestructor> {
    if !function.name.starts_with("__dt__")
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
    let [vptr_store, array_call, base_call, delete_guard] = then_body.as_slice() else {
        return None;
    };
    let own_vtable = parse_vptr_store(vptr_store)?;
    let (member_offset, element_destructor, element_size, element_count) =
        parse_array_call(array_call)?;
    let base_destructor = parse_base_call(base_call)?;
    let base = inline_summaries.trivial_virtual_destructor(base_destructor)?;
    if base.vptr_offset != 0 {
        return None;
    }
    let delete_callee = parse_delete_guard(delete_guard)?;
    Some(ArrayMemberDestructor {
        own_vtable,
        member_offset,
        element_destructor,
        element_size,
        element_count,
        base_vtable: base.vtable.clone(),
        delete_callee,
    })
}

fn parse_vptr_store(statement: &Statement) -> Option<String> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: 0,
                ..
            },
        value: Expression::AddressOf { operand },
    } = statement
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == "this") {
        return None;
    }
    let Expression::Variable(vtable) = operand.as_ref() else {
        return None;
    };
    Some(vtable.clone())
}

fn parse_array_call(statement: &Statement) -> Option<(i16, String, i16, i16)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [
        Expression::MemberAddress { base, offset, .. },
        Expression::AddressOf { operand },
        Expression::IntegerLiteral(element_size),
        Expression::IntegerLiteral(element_count),
    ] = arguments.as_slice()
    else {
        return None;
    };
    if name != "__destroy_arr"
        || !matches!(base.as_ref(), Expression::Variable(name) if name == "this")
    {
        return None;
    }
    let Expression::Variable(element_destructor) = operand.as_ref() else {
        return None;
    };
    Some((
        i16::try_from(*offset).ok()?,
        element_destructor.clone(),
        i16::try_from(*element_size).ok()?,
        i16::try_from(*element_count).ok()?,
    ))
}

fn parse_base_call(statement: &Statement) -> Option<&str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    if !matches!(
        arguments.as_slice(),
        [Expression::Variable(object), Expression::IntegerLiteral(0)] if object == "this"
    ) {
        return None;
    }
    Some(name)
}

fn parse_delete_guard(statement: &Statement) -> Option<String> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        || !matches!(right.as_ref(), Expression::IntegerLiteral(0))
        || !else_body.is_empty()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] = then_body.as_slice()
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [Expression::Variable(object)] if object == "this") {
        return None;
    }
    Some(name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn emits_the_linkage_first_array_member_lifetime_schedule() {
        let function = Function {
            return_type: Type::StructPointer { element_size: 4100 },
            name: "__dt__8DrumSetFv".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 4100 },
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
        let output = emit(
            &function,
            ArrayMemberDestructor {
                own_vtable: "__vt__8DrumSet".into(),
                member_offset: 4,
                element_destructor: "__dt__4PercFv".into(),
                element_size: 32,
                element_count: 128,
                base_vtable: "__vt__4Inst".into(),
                delete_callee: "__dl__FPv".into(),
            },
        );
        let actual = output
            .encode_text()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            actual,
            "7c0802a6900100049421ffe893e100143be4000093c100107c7e1b794182004c\
             3c600000380300003c600000901e000038830000387e000438a0002038c00080\
             48000001281e0000418200103c60000038030000901e00007fe007354081000c\
             7fc3f378480000018001001c7fc3f37883e1001483c100107c0803a638210018\
             4e800020"
        );
    }
}
