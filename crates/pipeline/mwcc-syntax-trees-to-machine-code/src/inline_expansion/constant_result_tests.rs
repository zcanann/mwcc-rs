use super::*;
use mwcc_syntax_trees::{BinaryOperator as B, GuardedReturn, Parameter, UnaryOperator as U};
use std::collections::HashMap;

fn function(name: &str) -> Function {
    Function {
        return_type: Type::Int,
        name: name.into(),
        is_static: true,
        is_weak: false,
        parameters: Vec::new(),
        locals: Vec::new(),
        statements: Vec::new(),
        guards: Vec::new(),
        return_expression: Some(Expression::IntegerLiteral(1)),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
        text_deferred: false,
        peephole_disabled: false,
    }
}

fn access() -> Expression {
    Expression::Index {
        base: Box::new(Expression::Variable("registers".into())),
        index: Box::new(Expression::IntegerLiteral(13)),
    }
}

fn poll() -> Function {
    let mut function = function("ready");
    function.statements.push(Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(access()),
        step: None,
        body: Vec::new(),
    });
    function
}

fn call() -> Expression {
    Expression::Call {
        name: "ready".into(),
        arguments: Vec::new(),
    }
}

#[test]
fn constant_result_exposure_preserves_short_circuit_and_early_exits() {
    let bodies = HashMap::from([("ready".into(), poll())]);
    let mut caller = function("caller");
    for operator in [B::LogicalAnd, B::LogicalOr] {
        caller.return_expression = Some(Expression::Binary {
            operator,
            left: Box::new(Expression::Variable("enabled".into())),
            right: Box::new(call()),
        });
        assert!(expose_values(&caller, &bodies).is_none());
    }
    caller.return_expression = Some(call());
    caller.guards.push(GuardedReturn {
        condition: Expression::Variable("disabled".into()),
        value: Expression::IntegerLiteral(0),
    });
    assert!(expose_values(&caller, &bodies).is_none());
}

#[test]
fn constant_result_exposure_stays_inside_its_conditional_arm() {
    let bodies = HashMap::from([("ready".into(), poll())]);
    let mut caller = function("caller");
    caller.statements.push(Statement::If {
        condition: Expression::Variable("enabled".into()),
        then_body: vec![Statement::Return(Some(call()))],
        else_body: Vec::new(),
    });
    let expanded = expose_values(&caller, &bodies).unwrap();
    assert!(matches!(expanded.statements.as_slice(), [Statement::If {
        then_body, else_body, ..
    }] if else_body.is_empty() && matches!(then_body.as_slice(), [
        Statement::Expression(Expression::Call { name, .. }),
        Statement::Return(Some(Expression::IntegerLiteral(1))),
    ] if name == "ready")));
}

#[test]
fn constant_result_exposure_rejects_bank_name_capture_and_eager_memory_siblings() {
    let bodies = HashMap::from([("ready".into(), poll())]);
    let mut caller = function("caller");
    caller.return_expression = Some(call());
    caller.parameters.push(Parameter {
        name: "registers".into(),
        parameter_type: Type::UnsignedInt,
    });
    assert!(expose_values(&caller, &bodies).is_none());
    caller.parameters.clear();
    caller.return_expression = Some(Expression::Binary {
        operator: B::BitOr,
        left: Box::new(access()),
        right: Box::new(call()),
    });
    assert!(expose_values(&caller, &bodies).is_none());
}

#[test]
fn constant_result_exposure_keeps_poll_when_status_accumulation_folds_away() {
    let bodies = HashMap::from([("ready".into(), poll())]);
    let mut caller = function("caller");
    caller.parameters.push(Parameter {
        name: "error".into(),
        parameter_type: Type::Int,
    });
    caller.statements.push(Statement::Assign {
        name: "error".into(),
        value: Expression::Binary {
            operator: B::BitOr,
            left: Box::new(Expression::Variable("error".into())),
            right: Box::new(Expression::Unary {
                operator: U::LogicalNot,
                operand: Box::new(call()),
            }),
        },
    });
    let expanded = expose_values(&caller, &bodies).unwrap();
    assert!(
        matches!(expanded.statements.as_slice(), [Statement::Expression(Expression::Call { name, .. })] if name == "ready")
    );
}

#[test]
fn constant_result_exposure_converts_status_to_its_declared_word_type() {
    let mut callee = poll();
    callee.return_expression = Some(Expression::IntegerLiteral(0x1_0000_0000));
    let bodies = HashMap::from([("ready".into(), callee)]);
    let mut caller = function("caller");
    caller.return_expression = Some(Expression::Unary {
        operator: U::LogicalNot,
        operand: Box::new(call()),
    });
    let expanded = expose_values(&caller, &bodies).unwrap();
    assert!(matches!(
        expanded.return_expression,
        Some(Expression::IntegerLiteral(1))
    ));
    assert_eq!(expanded.statements.len(), 1);
}

#[test]
fn constant_result_initializers_only_restore_fresh_entry_locals() {
    let name = "__mwcc_inline_ready_user".to_owned();
    let mut caller = function("caller");
    caller.locals.push(mwcc_syntax_trees::LocalDeclaration {
        declared_type: Type::UnsignedInt,
        name: name.clone(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    });
    caller.statements.push(Statement::Assign {
        name: name.clone(),
        value: access(),
    });
    let callees = vec!["ready".into()];
    restore_entry_initializers(
        &mut caller,
        &callees,
        &std::collections::HashSet::from([name]),
    );
    assert!(
        caller.locals[0].initializer.is_none(),
        "a user-chosen name is not an inline temporary"
    );
    assert_eq!(caller.statements.len(), 1);

    restore_entry_initializers(&mut caller, &callees, &std::collections::HashSet::new());
    assert!(matches!(
        caller.locals[0].initializer,
        Some(Expression::Index { .. })
    ));
    assert!(caller.statements.is_empty());
}
