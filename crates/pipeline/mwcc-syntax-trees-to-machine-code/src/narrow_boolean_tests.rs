use mwcc_machine_code::{Instruction as I, MachineFunction};
use mwcc_syntax_trees::{
    BinaryOperator as B, Expression as E, Function, Parameter, Statement, Type,
};
use mwcc_versions::{CompilerConfig, Optimization, GC_1_3};
use std::collections::HashMap;

fn member(offset: u32, ty: Type) -> E {
    E::Member {
        base: Box::new(E::Variable("p".into())),
        offset,
        member_type: ty,
        index_stride: None,
    }
}
fn binary(operator: B, left: E, right: E) -> E {
    E::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn function(result: Type, value: E) -> Function {
    Function {
        name: "boolean".into(),
        return_type: result,
        is_static: false,
        is_weak: false,
        parameters: vec![Parameter {
            name: "p".into(),
            parameter_type: Type::StructPointer { element_size: 12 },
        }],
        locals: vec![],
        statements: vec![],
        guards: vec![],
        return_expression: Some(value),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: vec![],
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}
fn lower(f: &Function) -> MachineFunction {
    let returns = HashMap::from([
        ("first".into(), Type::UnsignedInt),
        ("second".into(), Type::UnsignedInt),
        ("sink2".into(), Type::Void),
    ]);
    let params = HashMap::from([
        ("first".into(), vec![]),
        ("second".into(), vec![]),
        ("sink2".into(), vec![Type::UnsignedChar; 2]),
    ]);
    let mut config = CompilerConfig::new(GC_1_3);
    config.flags.optimization = Optimization::O4;
    config.flags.cpp_exceptions = false;
    crate::lower_function(
        f,
        &[],
        &HashMap::new(),
        &HashMap::new(),
        &returns,
        &params,
        &Default::default(),
        &Default::default(),
        &params.keys().cloned().collect(),
        &Default::default(),
        &HashMap::new(),
        &HashMap::new(),
        &crate::InlineBodySet::default(),
        &crate::InlineSummaries::default(),
        Default::default(),
        &HashMap::new(),
        &HashMap::new(),
        config,
    )
    .expect("narrow boolean should lower")
}
#[test]
fn unsigned_truth_return_fuses_width_into_the_boolean_shift() {
    for (ty, width) in [(Type::UnsignedChar, 8), (Type::UnsignedShort, 16)] {
        let f = lower(&function(
            ty,
            binary(
                B::Equal,
                member(0, Type::UnsignedChar),
                member(1, Type::UnsignedChar),
            ),
        ));
        assert!(f.instructions.iter().any(
            |i| matches!(i,I::RotateAndMask {a:3,shift:27,begin,end:31,..} if *begin==32-width)
        ));
        assert!(!f
            .instructions
            .iter()
            .any(|i| matches!(i, I::ExtendSignByte { .. } | I::ExtendSignHalfword { .. })));
    }
}
#[test]
fn signed_truth_return_retains_its_extension() {
    for ty in [Type::Char, Type::Short] {
        let f = lower(&function(
            ty,
            binary(
                B::Equal,
                member(0, Type::UnsignedChar),
                member(1, Type::UnsignedChar),
            ),
        ));
        assert!(f.instructions.iter().any(|i| match (ty, i) {
            (Type::Char, I::ExtendSignByte { a: 3, s: 0 })
            | (Type::Short, I::ExtendSignHalfword { a: 3, s: 0 }) => true,
            _ => false,
        }));
    }
}
#[test]
fn equality_calls_acquire_the_first_saved_home_in_an_existing_call_frame() {
    let call = |name: &str| E::Call {
        name: name.into(),
        arguments: vec![],
    };
    let mut f = function(
        Type::UnsignedChar,
        binary(B::Equal, call("first"), call("second")),
    );
    f.parameters.clear();
    let f = lower(&f);
    let calls = f
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(at, i)| {
            if let I::BranchAndLink { target } = i {
                Some((at, target.as_str()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        calls.iter().map(|(_, n)| *n).collect::<Vec<_>>(),
        ["second", "first"]
    );
    let save = f
        .instructions
        .iter()
        .position(|i| {
            matches!(
                i,
                I::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 12
                }
            )
        })
        .unwrap();
    let restore = f
        .instructions
        .iter()
        .position(|i| {
            matches!(
                i,
                I::LoadWord {
                    d: 31,
                    a: 1,
                    offset: 12
                }
            )
        })
        .unwrap();
    assert!(save < calls[0].0 && restore > calls[1].0);
    assert!(matches!(
        f.instructions[0],
        I::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16
        }
    ));
}
#[test]
fn short_circuit_preserves_its_shared_pointer_until_the_second_load() {
    let f = lower(&function(
        Type::UnsignedChar,
        binary(
            B::LogicalAnd,
            member(0, Type::UnsignedChar),
            member(1, Type::UnsignedChar),
        ),
    ));
    let second = f
        .instructions
        .iter()
        .position(|i| {
            matches!(
                i,
                I::LoadByteZero {
                    a: 3,
                    offset: 1,
                    ..
                }
            )
        })
        .unwrap();
    assert!(!f.instructions[..second]
        .iter()
        .flat_map(mwcc_vreg::register_operands)
        .any(|r| r.class == mwcc_vreg::Class::General
            && r.role == mwcc_vreg::RegisterRole::Define
            && r.register == 3));
}
#[test]
fn narrow_boolean_arguments_preserve_the_shared_member_base() {
    let condition = binary(
        B::Equal,
        member(2, Type::UnsignedShort),
        binary(
            B::Multiply,
            E::IntegerLiteral(2),
            member(4, Type::UnsignedShort),
        ),
    );
    let mut f = function(Type::Void, E::IntegerLiteral(0));
    f.return_expression = None;
    f.statements.push(Statement::Expression(E::Call {
        name: "sink2".into(),
        arguments: vec![member(0, Type::UnsignedChar), condition],
    }));
    let f = lower(&f);
    assert_eq!(
        f.instructions
            .iter()
            .filter(|i| matches!(i,I::BranchAndLink {target} if target=="sink2"))
            .count(),
        1
    );
    let last_load = f
        .instructions
        .iter()
        .rposition(|i| {
            matches!(
                i,
                I::LoadByteZero { a: 3, .. } | I::LoadHalfwordZero { a: 3, .. }
            )
        })
        .unwrap();
    assert!(!f.instructions[..last_load]
        .iter()
        .flat_map(mwcc_vreg::register_operands)
        .any(|r| r.class == mwcc_vreg::Class::General
            && r.role == mwcc_vreg::RegisterRole::Define
            && r.register == 3));
}
#[test]
fn boolean_range_recognition_does_not_classify_arithmetic_or_pointer_casts() {
    let eq = binary(B::Equal, E::Variable("x".into()), E::IntegerLiteral(0));
    assert!(crate::analysis::is_boolean_result(&eq));
    assert!(crate::analysis::is_boolean_result(&E::Cast {
        target_type: Type::UnsignedChar,
        operand: Box::new(eq.clone())
    }));
    assert!(!crate::analysis::is_boolean_result(&binary(
        B::Add,
        eq,
        E::IntegerLiteral(1)
    )));
    assert!(!crate::analysis::is_boolean_result(&E::IntegerLiteral(2)));
}
