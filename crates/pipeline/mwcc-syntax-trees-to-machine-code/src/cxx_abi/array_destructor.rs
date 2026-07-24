//! Optimized complete-object destructors whose lifetime work is `delete[]`.

use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention, Optimization};

struct ArrayDestructor {
    member_offsets: Vec<i16>,
    array_delete: String,
    scalar_delete: String,
}

/// Lower the legacy O4 schedule for a non-virtual destructor that releases
/// pointer members before running its complete-object deleting guard.
pub(crate) fn lower(
    function: &Function,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    let behavior = Behavior::resolve(&config);
    if behavior.frame_convention != FrameConvention::LinkageFirst
        || behavior.optimization != Optimization::O4
        || config.flags.cpp_exceptions
    {
        return None;
    }
    let shape = recognize(function)?;
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions.extend([
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
    ]);
    let null_branch = output.instructions.len();
    output
        .instructions
        .push(Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 0,
        });
    for offset in shape.member_offsets {
        output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset,
        });
        push_call(&mut output, &shape.array_delete);
    }
    output
        .instructions
        .push(Instruction::ExtendSignHalfwordRecord { a: 0, s: 31 });
    let deleting_branch = output.instructions.len();
    output
        .instructions
        .push(Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 0,
        });
    output
        .instructions
        .push(Instruction::move_register(3, 30));
    push_call(&mut output, &shape.scalar_delete);

    let epilogue = output.instructions.len();
    for branch in [null_branch, deleting_branch] {
        let Instruction::BranchConditionalForward { target, .. } =
            &mut output.instructions[branch]
        else {
            unreachable!("recorded a conditional branch");
        };
        *target = epilogue;
    }
    output.instructions.extend([
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
    ]);
    output.symbol_order = vec![shape.array_delete.clone(), shape.scalar_delete.clone()];
    output.referenced_function_symbols = output.symbol_order.clone();
    output.implicit_external_callees = output.symbol_order.clone();
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    Some(output)
}

fn push_call(output: &mut MachineFunction, target: &str) {
    let instruction_index = output.instructions.len();
    output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
    output.relocations.push(Relocation {
        instruction_index,
        kind: RelocationKind::Rel24,
        target: RelocationTarget::External(target.to_owned()),
    });
}

fn recognize(function: &Function) -> Option<ArrayDestructor> {
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
    let (delete_guard, lifetime) = then_body.split_last()?;
    let mut member_offsets = Vec::with_capacity(lifetime.len());
    let mut array_delete = None;
    for statement in lifetime {
        let Statement::Expression(Expression::Call { name, arguments }) = statement else {
            return None;
        };
        let [Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        }] = arguments.as_slice()
        else {
            return None;
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == "this")
            || !matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. })
        {
            return None;
        }
        match &array_delete {
            Some(existing) if existing != name => return None,
            None => array_delete = Some(name.clone()),
            _ => {}
        }
        member_offsets.push(i16::try_from(*offset).ok()?);
    }
    if member_offsets.is_empty() {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body: delete_body,
        else_body: delete_else,
    } = delete_guard
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: scalar_delete,
        arguments,
    })] = delete_body.as_slice()
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        || !matches!(right.as_ref(), Expression::IntegerLiteral(0))
        || !delete_else.is_empty()
        || !matches!(arguments.as_slice(), [Expression::Variable(name)] if name == "this")
    {
        return None;
    }
    Some(ArrayDestructor {
        member_offsets,
        array_delete: array_delete?,
        scalar_delete: scalar_delete.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Parameter, Pointee};

    #[test]
    fn recognizes_array_release_members_in_source_order() {
        let member_delete = |offset| {
            Statement::Expression(Expression::Call {
                name: "__dla__FPv".into(),
                arguments: vec![Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset,
                    member_type: Type::Pointer(Pointee::Int),
                    index_stride: None,
                }],
            })
        };
        let function = Function {
            return_type: Type::StructPointer { element_size: 32 },
            name: "__dt__5OwnerFv".into(),
            is_static: false,
            is_weak: false,
            text_deferred: false,
            peephole_disabled: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 32 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Short,
                    name: "__destroy".into(),
                },
            ],
            locals: Vec::new(),
            statements: vec![Statement::If {
                condition: Expression::Variable("this".into()),
                then_body: vec![
                    member_delete(16),
                    member_delete(28),
                    Statement::If {
                        condition: Expression::Binary {
                            operator: BinaryOperator::Greater,
                            left: Box::new(Expression::Variable("__destroy".into())),
                            right: Box::new(Expression::IntegerLiteral(0)),
                        },
                        then_body: vec![Statement::Expression(Expression::Call {
                            name: "__dl__FPv".into(),
                            arguments: vec![Expression::Variable("this".into())],
                        })],
                        else_body: Vec::new(),
                    },
                ],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("this".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
        };

        let shape = recognize(&function).expect("the canonical destructor should match");
        assert_eq!(shape.member_offsets, [16, 28]);
        assert_eq!(shape.array_delete, "__dla__FPv");
        assert_eq!(shape.scalar_delete, "__dl__FPv");
    }
}
