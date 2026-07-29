use std::collections::HashMap;

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, InlineExpansionFacts, Parameter, SourceFundamentalType,
    Type,
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
    let fundamentals = HashMap::from([(function.name.clone(), SourceFundamentalType::Boolean)]);
    let mut config = CompilerConfig::new(GC_1_2_5N);
    config.flags.cpp_exceptions = false;
    lower_function(
        function,
        &[],
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
    .expect("float comparison should lower")
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
