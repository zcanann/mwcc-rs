use std::collections::HashMap;

use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GlobalDeclaration, InlineExpansionFacts,
    LocalDeclaration, Parameter, SourceFundamentalType, Statement, Type,
};
use mwcc_versions::{CompilerConfig, GC_1_2_5N};

use crate::{lower_function, InlineBodySet, InlineSummaries};

fn function(name: &str, parameters: Vec<Parameter>, left: Expression) -> Function {
    Function {
        return_type: Type::UnsignedChar,
        name: name.into(),
        is_static: false,
        is_weak: true,
        parameters,
        locals: Vec::new(),
        statements: Vec::new(),
        guards: Vec::new(),
        return_expression: Some(Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(left),
            right: Box::new(Expression::FloatLiteral(0.0)),
        }),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}

fn lower(function: &Function) -> MachineFunction {
    lower_with_globals(function, &[])
}

fn lower_with_globals(function: &Function, globals: &[GlobalDeclaration]) -> MachineFunction {
    let fundamentals = HashMap::from([(function.name.clone(), SourceFundamentalType::Boolean)]);
    let mut config = CompilerConfig::new(GC_1_2_5N);
    config.flags.cpp_exceptions = false;
    lower_function(
        function,
        globals,
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &HashMap::new(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &Default::default(),
        &HashMap::new(),
        &HashMap::new(),
        &InlineBodySet::default(),
        &InlineSummaries::default(),
        InlineExpansionFacts::default(),
        &HashMap::new(),
        &fundamentals,
        config,
    )
    .expect("comparison should lower")
}

fn pointer_global(name: &str) -> GlobalDeclaration {
    typed_global(name, Type::StructPointer { element_size: 4 })
}

fn typed_global(name: &str, declared_type: Type) -> GlobalDeclaration {
    GlobalDeclaration {
        declared_type,
        source_fundamental: None,
        name: name.into(),
        is_extern: true,
        is_static: false,
        is_volatile: false,
        is_weak: false,
        force_active: false,
        non_static_functions_before: 0,
        functions_before: 0,
        array_length: None,
        array_length_inferred: false,
        initializer: None,
        is_const: false,
        pointer_pointee_const: false,
        address_initializer: None,
        data_bytes: None,
        data_relocations: Vec::new(),
        section: None,
        attribute_alignment: None,
    }
}

#[test]
fn narrow_memory_comparison_preserves_and_promotes_both_loads() {
    let function = Function {
        return_type: Type::Void,
        name: "same_state".into(),
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            parameter_type: Type::StructPointer { element_size: 80 },
            name: "object".into(),
        }],
        locals: Vec::new(),
        statements: vec![Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 72,
                    member_type: Type::Char,
                    index_stride: None,
                }),
                right: Box::new(Expression::Variable("state".into())),
            },
            then_body: vec![Statement::Expression(Expression::Call {
                name: "matched".into(),
                arguments: Vec::new(),
            })],
            else_body: Vec::new(),
        }],
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let machine = lower_with_globals(&function, &[typed_global("state", Type::Short)]);

    let byte_load = machine
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::LoadByteZero {
                    d: 0,
                    offset: 72,
                    ..
                }
            )
        })
        .expect("the signed member must load as a raw byte");
    let (extend, preserved) = machine.instructions[byte_load + 1..]
        .iter()
        .enumerate()
        .find_map(|(offset, instruction)| match instruction {
            Instruction::ExtendSignByte { a, s: 0 } if *a != 0 => {
                Some((byte_load + 1 + offset, *a))
            }
            _ => None,
        })
        .expect("the left byte must be promoted outside r0");
    let halfword_load = machine.instructions[extend + 1..]
        .iter()
        .position(|instruction| {
            matches!(instruction, Instruction::LoadHalfwordAlgebraic { d: 0, .. })
        })
        .map(|offset| extend + 1 + offset)
        .expect("the right signed short must reload through r0");
    assert!(
        machine.instructions[halfword_load + 1..]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::CompareWord { a, b: 0 } if *a == preserved)),
        "the promoted member and global short must be compared: {:?}",
        machine.instructions
    );
}

fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_be_bytes()).collect()
}

#[test]
fn loaded_float_comparison_extracts_cr_through_the_scratch() {
    let function = function(
        "isAlive__8CreatureFv",
        vec![Parameter {
            parameter_type: Type::StructPointer { element_size: 4 },
            name: "this".into(),
        }],
        Expression::Member {
            base: Box::new(Expression::Variable("this".into())),
            offset: 88,
            member_type: Type::Float,
            index_stride: None,
        },
    );

    assert_eq!(
        lower(&function).encode_text(),
        bytes(&[
            0xc023_0058,
            0xc000_0000,
            0xfc01_0040,
            0x7c00_0026,
            0x5403_17fe,
            0x4e80_0020,
        ])
    );
}

#[test]
fn register_float_comparison_extracts_cr_directly_into_the_result() {
    let function = function(
        "positive__Ff",
        vec![Parameter {
            parameter_type: Type::Float,
            name: "value".into(),
        }],
        Expression::Variable("value".into()),
    );

    assert_eq!(
        lower(&function).encode_text(),
        bytes(&[
            0xc000_0000,
            0xfc01_0040,
            0x7c60_0026,
            0x5463_17fe,
            0x4e80_0020,
        ])
    );
}

#[test]
fn two_global_comparison_preserves_both_loaded_values() {
    let function = Function {
        return_type: Type::Void,
        name: "different".into(),
        is_static: false,
        is_weak: false,
        parameters: Vec::new(),
        locals: Vec::new(),
        statements: vec![Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(Expression::Variable("current".into())),
                right: Box::new(Expression::Variable("prior".into())),
            },
            then_body: vec![Statement::Expression(Expression::Call {
                name: "changed".into(),
                arguments: Vec::new(),
            })],
            else_body: Vec::new(),
        }],
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let machine = lower_with_globals(
        &function,
        &[pointer_global("current"), pointer_global("prior")],
    );

    assert!(
        machine.instructions.windows(3).any(|window| {
            window
                == [
                    Instruction::LoadWord {
                        d: 3,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::LoadWord {
                        d: 0,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::CompareLogicalWord { a: 3, b: 0 },
                ]
        }),
        "two direct globals must not collapse to a self-comparison: {:?}",
        machine.instructions
    );
}

#[test]
fn shifted_wide_add_compares_from_the_scratch() {
    let function = Function {
        return_type: Type::Void,
        name: "f".into(),
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            parameter_type: Type::UnsignedInt,
            name: "x".into(),
        }],
        locals: Vec::new(),
        statements: vec![Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("x".into())),
                    right: Box::new(Expression::IntegerLiteral(0x232f_0000)),
                }),
                right: Box::new(Expression::IntegerLiteral(2)),
            },
            then_body: vec![Statement::Expression(Expression::Call {
                name: "g".into(),
                arguments: Vec::new(),
            })],
            else_body: Vec::new(),
        }],
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let machine = lower(&function);

    assert!(
        machine.instructions.windows(2).any(|window| {
            window
                == [
                    Instruction::AddImmediateShifted {
                        d: 0,
                        a: 3,
                        immediate: 0x232f,
                    },
                    Instruction::CompareLogicalWordImmediate {
                        a: 0,
                        immediate: 2,
                    },
                ]
        }),
        "the shifted add must remain directly comparable in r0: {:?}",
        machine.instructions
    );
}

#[test]
fn address_taken_scalar_large_equality_uses_a_nonzero_addis_source() {
    let function = Function {
        return_type: Type::Void,
        name: "f".into(),
        is_static: false,
        is_weak: false,
        parameters: Vec::new(),
        locals: vec![LocalDeclaration {
            declared_type: Type::UnsignedInt,
            name: "word".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }],
        statements: vec![
            Statement::Expression(Expression::Call {
                name: "fill".into(),
                arguments: vec![Expression::AddressOf {
                    operand: Box::new(Expression::Variable("word".into())),
                }],
            }),
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("word".into())),
                    right: Box::new(Expression::IntegerLiteral(0x0fe0_0000)),
                },
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "matched".into(),
                    arguments: Vec::new(),
                })],
                else_body: Vec::new(),
            },
        ],
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let machine = lower(&function);

    assert!(
        machine.instructions.windows(3).any(|window| matches!(
            window,
            [
                Instruction::LoadWord { d: 3, a: 1, .. },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 3,
                    immediate: -4064,
                },
                Instruction::CompareLogicalWordImmediate {
                    a: 0,
                    immediate: 0,
                },
            ]
        )),
        "the frame reload must feed addis through a non-r0 source: {:?}",
        machine.instructions
    );
}

#[test]
fn indexes_through_a_memory_backed_global_pointer_table() {
    let function = Function {
        return_type: Type::Pointer(mwcc_syntax_trees::Pointee::Char),
        name: "lookup".into(),
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            parameter_type: Type::UnsignedInt,
            name: "offset".into(),
        }],
        locals: Vec::new(),
        statements: vec![Statement::Store {
            target: Expression::Index {
                base: Box::new(Expression::Cast {
                    target_type: Type::Pointer(mwcc_syntax_trees::Pointee::Char),
                    operand: Box::new(Expression::Index {
                        base: Box::new(Expression::Variable("table".into())),
                        index: Box::new(Expression::Variable("index".into())),
                    }),
                }),
                index: Box::new(Expression::Variable("offset".into())),
            },
            value: Expression::IntegerLiteral(0),
        }],
        guards: Vec::new(),
        return_expression: Some(Expression::Index {
            base: Box::new(Expression::Variable("table".into())),
            index: Box::new(Expression::Variable("index".into())),
        }),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let machine = lower_with_globals(
        &function,
        &[
            typed_global(
                "table",
                Type::Pointer(mwcc_syntax_trees::Pointee::Pointer),
            ),
            typed_global("index", Type::UnsignedInt),
        ],
    );

    assert!(
        machine
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::LoadWordIndexed { .. })),
        "the loaded global pointer must feed an indexed word load: {:?}",
        machine.instructions
    );
    assert!(
        machine
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::StoreByteIndexed { .. })),
        "the intermediate char pointer must support a variable-index zero store: {:?}",
        machine.instructions
    );
}
