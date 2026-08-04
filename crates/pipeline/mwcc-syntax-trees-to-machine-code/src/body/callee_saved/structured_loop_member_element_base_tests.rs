use super::*;

fn indexed_member(offset: u32, member_type: Type, index: &str) -> Expression {
    Expression::Index {
        base: Box::new(match member_type {
            Type::Struct { .. } => Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset,
                member_type,
                index_stride: None,
            },
            Type::UnsignedShort => Expression::MemberAddress {
                base: Box::new(Expression::Variable("object".into())),
                offset,
                element: Pointee::UnsignedShort,
                index_stride: None,
            },
            _ => unreachable!(),
        }),
        index: Box::new(Expression::Variable(index.into())),
    }
}

fn body() -> Vec<Statement> {
    vec![
        Statement::Assign {
            name: "record".into(),
            value: indexed_member(
                264,
                Type::Struct { size: 2, align: 2 },
                &prescaled(),
            ),
        },
        Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![Statement::Store {
                target: indexed_member(276, Type::UnsignedShort, "index"),
                value: Expression::IntegerLiteral(0),
            }],
            else_body: Vec::new(),
        },
        Statement::Expression(Expression::Call {
            name: "transform".into(),
            arguments: Vec::new(),
        }),
        Statement::Store {
            target: indexed_member(276, Type::UnsignedShort, "index"),
            value: Expression::IntegerLiteral(1),
        },
    ]
}

fn prescaled() -> String {
    format!(
        "{}0",
        crate::analysis::PRESCALED_MEMBER_ARRAY_INDEX_PREFIX
    )
}

fn initializer() -> Expression {
    Expression::Comma {
        left: Box::new(Expression::Assign {
            target: Box::new(Expression::Variable("index".into())),
            value: Box::new(Expression::IntegerLiteral(0)),
        }),
        right: Box::new(Expression::Assign {
            target: Box::new(Expression::Variable(prescaled())),
            value: Box::new(Expression::IntegerLiteral(0)),
        }),
    }
}

fn step() -> Expression {
    let increment = |name: String, amount| Expression::Assign {
        target: Box::new(Expression::Variable(name.clone())),
        value: Box::new(Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable(name)),
            right: Box::new(Expression::IntegerLiteral(amount)),
        }),
    };
    Expression::Comma {
        left: Box::new(increment("index".into(), 1)),
        right: Box::new(increment(prescaled(), 2)),
    }
}

#[test]
fn shares_one_element_base_across_record_reads_and_writes() {
    let plan = Plan::recognize(Some(&initializer()), Some(&step()), &body())
        .expect("member arrays should share an element base");
    assert_eq!(plan.stride, 2);

    let cursor_value = plan.cursor_value();
    assert!(matches!(
        cursor_value,
        Expression::AddressOf { operand }
            if matches!(operand.as_ref(), Expression::Index { .. })
    ));
    let rewritten: Vec<_> = body()
        .iter()
        .map(|statement| plan.rewrite_statement(statement, "cursor"))
        .collect();
    assert!(matches!(
        &rewritten[0],
        Statement::Assign {
            value: Expression::Index { base, index },
            ..
        } if matches!(base.as_ref(), Expression::Member { base, offset: 264, .. }
            if matches!(base.as_ref(), Expression::Variable(name) if name == "cursor"))
            && crate::analysis::constant_value(index) == Some(0)
    ));
    assert!(matches!(
        &rewritten[3],
        Statement::Store {
            target: Expression::Member { base, offset: 276, .. },
            ..
        } if matches!(base.as_ref(), Expression::Variable(name) if name == "cursor")
    ));
}

#[test]
fn requires_three_accesses_across_distinct_member_offsets() {
    let mut too_small = body();
    too_small.pop();
    assert!(Plan::recognize(Some(&initializer()), Some(&step()), &too_small).is_none());
}

#[test]
fn requires_a_matching_prescaled_induction_step() {
    let wrong_step = Expression::Comma {
        left: Box::new(Expression::Assign {
            target: Box::new(Expression::Variable("index".into())),
            value: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("index".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        }),
        right: Box::new(Expression::Assign {
            target: Box::new(Expression::Variable(prescaled())),
            value: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable(prescaled())),
                right: Box::new(Expression::IntegerLiteral(4)),
            }),
        }),
    };
    assert!(Plan::recognize(Some(&initializer()), Some(&wrong_step), &body()).is_none());
}
