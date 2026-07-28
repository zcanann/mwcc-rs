//! Call-surviving scaled indices for repeated global struct-array accesses.
//!
//! This is a source-level value plan, not an instruction peephole. A stable
//! parameter subscript used repeatedly across calls owns one scaled-index live
//! range; expression lowering still owns each global base and displacement.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalIndexPlan {
    pub(super) global: String,
    pub(super) index: String,
    pub(super) stride: u32,
}

pub(super) fn plan(
    function: &Function,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalIndexPlan> {
    let parameters: std::collections::HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect();
    let mut counts = std::collections::HashMap::<(String, String, u32), usize>::new();
    for statement in &function.statements {
        visit_statement(statement, &mut |global, index| {
            if !parameters.contains(index) || !global_array_sizes.contains_key(global) {
                return;
            }
            let Some(Type::Struct { size, .. }) = globals.get(global).copied() else {
                return;
            };
            if size != 0 {
                *counts
                    .entry((global.to_owned(), index.to_owned(), u32::from(size)))
                    .or_default() += 1;
            }
        });
    }
    let ((global, index, stride), count) =
        counts.into_iter().max_by_key(|(_, count)| *count)?;
    if count < 3 || statements_assign_name(&function.statements, &index) {
        return None;
    }
    Some(StructuredGlobalIndexPlan {
        global,
        index,
        stride,
    })
}

fn visit_statement(statement: &Statement, visit: &mut impl FnMut(&str, &str)) {
    match statement {
        Statement::Store { target, value } => {
            visit_expression(target, visit);
            visit_expression(value, visit);
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            visit_expression(value, visit);
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            visit_expression(condition, visit);
            for statement in then_body.iter().chain(else_body) {
                visit_statement(statement, visit);
            }
        }
        Statement::Return(value) => {
            if let Some(value) = value {
                visit_expression(value, visit);
            }
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            visit_expression(scrutinee, visit);
            for arm in arms {
                visit_arm_body(&arm.body, visit);
            }
            if let Some(default) = default {
                visit_arm_body(default, visit);
            }
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            for expression in [initializer, condition, step].into_iter().flatten() {
                visit_expression(expression, visit);
            }
            for statement in body {
                visit_statement(statement, visit);
            }
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => {}
    }
}

fn visit_arm_body(body: &ArmBody, visit: &mut impl FnMut(&str, &str)) {
    match body {
        ArmBody::Return(expression) => visit_expression(expression, visit),
        ArmBody::Statements(statements) => {
            for statement in statements {
                visit_statement(statement, visit);
            }
        }
    }
}

fn visit_expression(expression: &Expression, visit: &mut impl FnMut(&str, &str)) {
    if let Expression::MemberAddress { base, .. } = expression {
        if let Expression::Index { base, index } = base.as_ref() {
            if let (Expression::Variable(global), Expression::Variable(index)) =
                (base.as_ref(), index.as_ref())
            {
                visit(global, index);
            }
        }
    }
    match expression {
        Expression::AggregateLiteral(elements) => {
            for element in elements {
                visit_expression(element, visit);
            }
        }
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            visit_expression(left, visit);
            visit_expression(right, visit);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            visit_expression(condition, visit);
            visit_expression(when_true, visit);
            visit_expression(when_false, visit);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => visit_expression(operand, visit),
        Expression::BitFieldRead {
            extracted, storage, ..
        }
        | Expression::Index {
            base: extracted,
            index: storage,
        } => {
            visit_expression(extracted, visit);
            visit_expression(storage, visit);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            visit_expression(base, visit);
        }
        Expression::CallThrough { target, arguments } => {
            visit_expression(target, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            visit_expression(object, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            visit_expression(allocation, visit);
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                visit_expression(argument, visit);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

fn statements_assign_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: target, .. } => target == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statements_assign_name(then_body, name)
                || statements_assign_name(else_body, name)
        }
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| arm_body_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_assigns_name(body, name))
        }
        Statement::Loop { body, .. } => statements_assign_name(body, name),
        _ => false,
    })
}

fn arm_body_assigns_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(_) => false,
        ArmBody::Statements(statements) => statements_assign_name(statements, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Function, Parameter};

    fn member(offset: u32) -> Expression {
        Expression::MemberAddress {
            base: Box::new(Expression::Index {
                base: Box::new(Expression::Variable("records".into())),
                index: Box::new(Expression::Variable("i".into())),
            }),
            offset,
            element: mwcc_syntax_trees::Pointee::UnsignedChar,
            index_stride: None,
        }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "i".into(),
            }],
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn maps() -> (
        std::collections::HashMap<String, Type>,
        std::collections::HashMap<String, u32>,
    ) {
        (
            std::collections::HashMap::from([(
                "records".into(),
                Type::Struct {
                    size: 108,
                    align: 4,
                },
            )]),
            std::collections::HashMap::from([("records".into(), 864)]),
        )
    }

    #[test]
    fn plans_a_repeated_stable_parameter_subscript() {
        let function = function(vec![Statement::Expression(Expression::Call {
            name: "sink".into(),
            arguments: vec![member(0), member(4), member(8)],
        })]);
        let (globals, sizes) = maps();
        assert_eq!(
            plan(&function, &globals, &sizes),
            Some(StructuredGlobalIndexPlan {
                global: "records".into(),
                index: "i".into(),
                stride: 108,
            })
        );
    }

    #[test]
    fn rejects_an_index_reassigned_in_the_body() {
        let function = function(vec![
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: vec![member(0), member(4), member(8)],
            }),
            Statement::Assign {
                name: "i".into(),
                value: Expression::IntegerLiteral(0),
            },
        ]);
        let (globals, sizes) = maps();
        assert_eq!(plan(&function, &globals, &sizes), None);
    }
}
