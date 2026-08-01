//! Strength reduction for a global byte table indexed by a counted loop.
//!
//! `global.bytes[i]` with a unit-step induction variable is an affine address.
//! Optimized MWCC retains the global base in a moving cursor, loads the byte at
//! the member displacement, and increments the cursor once per iteration. This
//! source-tree normalization exposes that additional loop-carried value to the
//! ordinary saved-home and liveness machinery.

use super::*;

pub(super) fn strength_reduce_global_byte_loop_cursor(
    function: &Function,
) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_name = 0usize;
    let mut declarations = Vec::new();
    let mut statements = Vec::with_capacity(function.statements.len());
    let mut changed = false;

    for statement in &function.statements {
        let Some(plan) = GlobalByteCursorPlan::recognize(statement) else {
            statements.push(statement.clone());
            continue;
        };
        let name = fresh_name(&mut used, &mut next_name);
        declarations.push(LocalDeclaration {
            declared_type: Type::Pointer(Pointee::UnsignedChar),
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
        statements.push(Statement::Assign {
            name: name.clone(),
            value: Expression::Cast {
                target_type: Type::Pointer(Pointee::UnsignedChar),
                operand: Box::new(Expression::AddressOf {
                    operand: Box::new(Expression::Variable(plan.global.clone())),
                }),
            },
        });
        statements.push(plan.rewrite(statement, &name));
        changed = true;
    }

    changed.then(|| {
        let mut reduced = function.clone();
        reduced.locals.extend(declarations);
        reduced.statements = statements;
        reduced
    })
}

struct GlobalByteCursorPlan {
    global: String,
    member_offset: u32,
}

impl GlobalByteCursorPlan {
    fn recognize(statement: &Statement) -> Option<Self> {
        let Statement::Loop {
            condition: Some(condition),
            step: Some(step),
            body,
            ..
        } = statement
        else {
            return None;
        };
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } = condition
        else {
            return None;
        };
        let Expression::Variable(index) = left.as_ref() else {
            return None;
        };
        let Expression::Member {
            base: count_base,
            index_stride: None,
            ..
        } = right.as_ref()
        else {
            return None;
        };
        let Expression::Variable(global) = count_base.as_ref() else {
            return None;
        };
        if !is_unit_step(step, index) {
            return None;
        }
        let Statement::Switch { scrutinee, .. } = body.first()? else {
            return None;
        };
        let Expression::Index { base, index: used_index } = scrutinee else {
            return None;
        };
        let Expression::MemberAddress {
            base: byte_base,
            offset,
            element: Pointee::UnsignedChar,
            index_stride: None,
        } = base.as_ref()
        else {
            return None;
        };
        if !matches!(byte_base.as_ref(), Expression::Variable(name) if name == global)
            || !matches!(used_index.as_ref(), Expression::Variable(name) if name == index)
        {
            return None;
        }
        Some(Self {
            global: global.clone(),
            member_offset: *offset,
        })
    }

    fn rewrite(&self, statement: &Statement, cursor: &str) -> Statement {
        let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        else {
            unreachable!("cursor plan was recognized from a loop")
        };
        let mut body = body.clone();
        let Statement::Switch {
            scrutinee: _,
            arms,
            default,
        } = &body[0]
        else {
            unreachable!("cursor plan was recognized from a leading switch")
        };
        body[0] = Statement::Switch {
            scrutinee: Expression::Index {
                base: Box::new(Expression::Variable(cursor.to_owned())),
                index: Box::new(Expression::IntegerLiteral(i64::from(self.member_offset))),
            },
            arms: arms.clone(),
            default: default.clone(),
        };
        let cursor_step = Expression::Assign {
            target: Box::new(Expression::Variable(cursor.to_owned())),
            value: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable(cursor.to_owned())),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        };
        Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: Some(Expression::Comma {
                left: Box::new(step.clone().expect("recognized loop step")),
                right: Box::new(cursor_step),
            }),
            body,
        }
    }
}

fn is_unit_step(step: &Expression, index: &str) -> bool {
    matches!(
        step,
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == index)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if matches!(left.as_ref(), Expression::Variable(name) if name == index)
                    && constant_value(right) == Some(1))
    )
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next: &mut usize) -> String {
    loop {
        let name = format!("__mwcc_global_byte_cursor_{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}
