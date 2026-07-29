use std::collections::HashMap;

use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GuardedReturn, InlineExpansionFacts, Parameter,
    SourceFundamentalType, Type,
};
use mwcc_versions::{CompilerConfig, GC_1_2_5N};

use crate::{lower_function, InlineBodySet, InlineSummaries};

fn variable(name: &str) -> Expression {
    Expression::Variable(name.into())
}

fn logical_and(left: Expression, right: Expression) -> Expression {
    Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn member(base: &str, offset: u32) -> Expression {
    Expression::Member {
        base: Box::new(variable(base)),
        offset,
        member_type: Type::StructPointer { element_size: 4 },
        index_stride: None,
    }
}

#[test]
fn linkage_first_guarded_virtual_call_retains_two_entry_parameters() {
    let alive = Expression::VirtualCall {
        object: Box::new(variable("target")),
        vptr_offset: 0,
        slot_offset: 136,
        return_type: Type::UnsignedChar,
        variadic: false,
        arguments: Vec::new(),
    };
    let attached_to_boss = Expression::Binary {
        operator: BinaryOperator::Equal,
        left: Box::new(member("target", 388)),
        right: Box::new(member("this", 4)),
    };
    let function = Function {
        return_type: Type::UnsignedChar,
        name: "satisfy__16CndStickBossKillFP8Creature".into(),
        is_static: false,
        is_weak: false,
        parameters: vec![
            Parameter {
                parameter_type: Type::StructPointer { element_size: 4 },
                name: "this".into(),
            },
            Parameter {
                parameter_type: Type::StructPointer { element_size: 4 },
                name: "target".into(),
            },
        ],
        locals: Vec::new(),
        statements: Vec::new(),
        guards: vec![GuardedReturn {
            condition: logical_and(variable("target"), logical_and(alive, attached_to_boss)),
            value: Expression::IntegerLiteral(1),
        }],
        return_expression: Some(Expression::IntegerLiteral(0)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    };
    let fundamentals = HashMap::from([(function.name.clone(), SourceFundamentalType::Boolean)]);
    let mut config = CompilerConfig::new(GC_1_2_5N);
    config.flags.cpp_exceptions = false;

    let output = lower_function(
        &function,
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
    .expect("guarded structured virtual call should lower");
    let expected = [
        0x7c08_02a6,
        0x9001_0004,
        0x9421_ffe0,
        0x93e1_001c,
        0x7c9f_2379,
        0x93c1_0018,
        0x3bc3_0000,
        0x4182_0038,
        0x7fe3_fb78,
        0x819f_0000,
        0x818c_0088,
        0x7d88_03a6,
        0x4e80_0021,
        0x5460_063f,
        0x4182_001c,
        0x807f_0184,
        0x801e_0004,
        0x7c03_0040,
        0x4082_000c,
        0x3860_0001,
        0x4800_0008,
        0x3860_0000,
        0x8001_0024,
        0x83e1_001c,
        0x83c1_0018,
        0x3821_0020,
        0x7c08_03a6,
        0x4e80_0020,
    ]
    .into_iter()
    .flat_map(u32::to_be_bytes)
    .collect::<Vec<_>>();

    assert_eq!(output.encode_text(), expected);
}
