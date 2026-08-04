use super::*;
use mwcc_syntax_trees::{LoopKind, Parameter, Pointee};

fn member(owner: &str, offset: u32) -> Expression {
    Expression::Member {
        base: Box::new(Expression::Variable(owner.into())),
        offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    }
}

fn condition(direction: Direction) -> Expression {
    let (operator, left, right) = match direction {
        Direction::Enqueue => (
            BinaryOperator::LessEqual,
            member("queue", 20),
            member("queue", 28),
        ),
        Direction::Dequeue => (
            BinaryOperator::Equal,
            member("queue", 28),
            Expression::IntegerLiteral(0),
        ),
    };
    Expression::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}

fn function(direction: Direction) -> Function {
    Function {
        return_type: Type::Int,
        name: "queue_transaction".into(),
        is_static: false,
        is_weak: false,
        parameters: vec![
            Parameter {
                name: "queue".into(),
                parameter_type: Type::Pointer(Pointee::Int),
            },
            Parameter {
                name: "payload".into(),
                parameter_type: Type::Pointer(Pointee::Int),
            },
            Parameter {
                name: "flags".into(),
                parameter_type: Type::Int,
            },
        ],
        locals: Vec::new(),
        statements: vec![
            Statement::Assign {
                name: "interrupt".into(),
                value: Expression::Call {
                    name: "disable_interrupts".into(),
                    arguments: Vec::new(),
                },
            },
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(condition(direction)),
                step: None,
                body: Vec::new(),
            },
        ],
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

#[test]
fn recognizes_both_queue_transaction_directions() {
    for expected in [Direction::Enqueue, Direction::Dequeue] {
        let plan = StructuredBlockingQueueTransaction::plan(&function(expected))
            .expect("the queue transaction should be recognized");
        assert_eq!(plan.direction, expected);
    }
}

#[test]
fn rejects_a_similar_loop_with_the_wrong_queue_member() {
    let mut function = function(Direction::Dequeue);
    let Statement::Loop {
        condition: Some(Expression::Binary { left, .. }),
        ..
    } = &mut function.statements[1]
    else {
        unreachable!()
    };
    *left = Box::new(member("queue", 24));

    assert!(StructuredBlockingQueueTransaction::plan(&function).is_none());
}

#[test]
fn assigns_roles_and_save_order_independently_of_source_order() {
    let enqueue = StructuredBlockingQueueTransaction::plan(&function(Direction::Enqueue)).unwrap();
    assert_eq!(enqueue.preference("queue"), Some(28));
    assert_eq!(enqueue.preference("payload"), Some(29));
    assert_eq!(enqueue.preference("interrupt"), Some(30));
    assert_eq!(enqueue.preference("flags"), Some(31));

    let homes = StructuredBlockingQueueHomes {
        owner: 40,
        payload: 41,
        flags: 42,
        interrupt: 43,
    };
    assert_eq!(enqueue.save_order(homes), [42, 43, 41, 40]);

    let dequeue = StructuredBlockingQueueTransaction::plan(&function(Direction::Dequeue)).unwrap();
    assert_eq!(dequeue.save_order(homes), [40, 42, 43, 41]);
}
