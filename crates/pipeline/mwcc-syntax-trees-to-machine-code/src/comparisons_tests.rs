use std::collections::HashMap;

use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GlobalDeclaration, InlineExpansionFacts, Parameter,
    SourceFundamentalType, Statement, Type,
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
    GlobalDeclaration {
        declared_type: Type::StructPointer { element_size: 4 },
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
