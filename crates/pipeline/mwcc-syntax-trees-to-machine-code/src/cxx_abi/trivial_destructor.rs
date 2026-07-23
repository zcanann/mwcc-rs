//! Optimized schedules for a source-written destructor with no lifetime work.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::{
    Behavior, CompilerConfig, CxxTrivialDestructorStyle, FrameConvention, Optimization,
};

/// Lower the canonical complete-object wrapper whose only action is the
/// deleting guard. O0/O1 retain stack homes and duplicate null tests; those
/// larger schedules remain with ordinary lowering until modeled explicitly.
pub(crate) fn lower(function: &Function, config: CompilerConfig) -> Option<MachineFunction> {
    let deleting_callee = match_deleting_shell(function)?;
    let behavior = Behavior::resolve(&config);
    if behavior.optimization < Optimization::O2 {
        return None;
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = match (
        behavior.frame_convention,
        behavior.cxx_trivial_destructor_style,
    ) {
        (FrameConvention::LinkageFirst, CxxTrivialDestructorStyle::RecordTests) => {
            postincrement_record_schedule(behavior.optimization, &deleting_callee)
        }
        (FrameConvention::Predecrement, CxxTrivialDestructorStyle::RecordTests) => {
            predecrement_record_schedule(behavior.optimization, &deleting_callee)
        }
        (FrameConvention::Predecrement, CxxTrivialDestructorStyle::ExplicitTests) => {
            predecrement_explicit_schedule(behavior.optimization, &deleting_callee)
        }
        (FrameConvention::LinkageFirst, CxxTrivialDestructorStyle::ExplicitTests) => return None,
    };
    let call_index = output
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))?;
    output.relocations = vec![Relocation {
        instruction_index: call_index,
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
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 1,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}

fn match_deleting_shell(function: &Function) -> Option<String> {
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
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body: delete_body,
        else_body: delete_else,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call { name, arguments })] = delete_body.as_slice()
    else {
        return None;
    };
    (condition == "this"
        && else_body.is_empty()
        && matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        && matches!(right.as_ref(), Expression::IntegerLiteral(0))
        && delete_else.is_empty()
        && matches!(arguments.as_slice(), [Expression::Variable(name)] if name == "this"))
    .then(|| name.clone())
}

fn postincrement_record_schedule(
    optimization: Optimization,
    deleting_callee: &str,
) -> Vec<Instruction> {
    let mut instructions = vec![
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
            target: 10,
        },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 10,
        },
        Instruction::move_register(3, 31),
        Instruction::BranchAndLink {
            target: deleting_callee.to_owned(),
        },
    ];
    if optimization == Optimization::O4 {
        instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::move_register(3, 31),
        ]);
    } else {
        instructions.extend([
            Instruction::move_register(3, 31),
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
        ]);
    }
    instructions.extend([
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
    ]);
    instructions
}

fn predecrement_record_schedule(
    optimization: Optimization,
    deleting_callee: &str,
) -> Vec<Instruction> {
    let mut instructions = compact_predecrement_prefix();
    instructions.extend([
        Instruction::OrRecord { a: 31, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 9,
        },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 9,
        },
        Instruction::BranchAndLink {
            target: deleting_callee.to_owned(),
        },
    ]);
    if optimization == Optimization::O4 {
        instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::move_register(3, 31),
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
        ]);
    } else {
        instructions.extend([
            Instruction::move_register(3, 31),
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
        ]);
    }
    instructions.extend(compact_predecrement_suffix());
    instructions
}

fn predecrement_explicit_schedule(
    optimization: Optimization,
    deleting_callee: &str,
) -> Vec<Instruction> {
    let mut instructions = vec![
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
    ];
    if optimization == Optimization::O4 {
        instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    }
    instructions.extend([
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
        Instruction::move_register(31, 3),
    ]);
    if optimization != Optimization::O4 {
        instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
    }
    instructions.push(Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: 10,
    });
    instructions.push(if optimization == Optimization::O2 {
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 }
    } else {
        Instruction::CompareWordImmediate { a: 4, immediate: 0 }
    });
    instructions.extend([
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 10,
        },
        Instruction::BranchAndLink {
            target: deleting_callee.to_owned(),
        },
        Instruction::move_register(3, 31),
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
    ]);
    instructions.extend(compact_predecrement_suffix());
    instructions
}

fn compact_predecrement_prefix() -> Vec<Instruction> {
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
    ]
}

fn compact_predecrement_suffix() -> [Instruction; 3] {
    [
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;
    use mwcc_versions::{CompilerBuild, CompilerConfig, GC_1_2_5, GC_2_7, GC_3_0A3};

    fn destructor() -> Function {
        Function {
            return_type: Type::StructPointer { element_size: 4 },
            name: "__dt__5PlainFv".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 4 },
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
                then_body: vec![Statement::If {
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
                }],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("this".into())),
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
    fn optimized_schedules_match_the_cross_version_oracle_matrix() {
        let function = destructor();
        let cases: &[(CompilerBuild, Optimization, &str)] = &[
            (GC_1_2_5, Optimization::O2, "7c0802a6900100049421ffe893e100147c7f1b79418200147c8007354081000c7fe3fb78480000017fe3fb788001001c83e10014382100187c0803a64e800020"),
            (GC_1_2_5, Optimization::O3, "7c0802a6900100049421ffe893e100147c7f1b79418200147c8007354081000c7fe3fb78480000017fe3fb788001001c83e10014382100187c0803a64e800020"),
            (GC_1_2_5, Optimization::O4, "7c0802a6900100049421ffe893e100147c7f1b79418200147c8007354081000c7fe3fb78480000018001001c7fe3fb7883e10014382100187c0803a64e800020"),
            (GC_2_7, Optimization::O2, "9421fff07c0802a69001001493e1000c7c7f1b79418200107c80073540810008480000017fe3fb7883e1000c800100147c0803a6382100104e800020"),
            (GC_2_7, Optimization::O3, "9421fff07c0802a69001001493e1000c7c7f1b79418200107c80073540810008480000017fe3fb7883e1000c800100147c0803a6382100104e800020"),
            (GC_2_7, Optimization::O4, "9421fff07c0802a69001001493e1000c7c7f1b79418200107c8007354081000848000001800100147fe3fb7883e1000c7c0803a6382100104e800020"),
            (GC_3_0A3, Optimization::O2, "9421fff07c0802a69001001493e1000c7c7f1b782c030000418200107c80073540810008480000017fe3fb7883e1000c800100147c0803a6382100104e800020"),
            (GC_3_0A3, Optimization::O3, "9421fff07c0802a69001001493e1000c7c7f1b782c030000418200102c04000040810008480000017fe3fb7883e1000c800100147c0803a6382100104e800020"),
            (GC_3_0A3, Optimization::O4, "9421fff07c0802a62c0300009001001493e1000c7c7f1b78418200102c04000040810008480000017fe3fb7883e1000c800100147c0803a6382100104e800020"),
        ];

        for (build, optimization, expected) in cases {
            let mut config = CompilerConfig::new(*build);
            config.flags.optimization = *optimization;
            let output = lower(&function, config).unwrap();
            let actual = output
                .encode_text()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(&actual, expected, "{} {optimization:?}", build.label);
        }
    }
}
