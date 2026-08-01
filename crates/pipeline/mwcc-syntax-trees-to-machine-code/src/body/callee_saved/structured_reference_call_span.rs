//! Path-sensitive reference/call ordering for structured frame plans.
//!
//! A callee-saved value is only justified when its source value can be used on
//! both sides of a call.  Counting references and calls independently loses
//! that lifetime boundary, especially in guarded event dispatchers whose final
//! global load is an argument to their first call.

use mwcc_syntax_trees::{ArmBody, Expression, Function, LoopKind, Statement};

#[derive(Clone, Copy, Default)]
struct Flow {
    has_reference: bool,
    has_call: bool,
    reference_before_call: bool,
    call_before_reference: bool,
    reference_call_reference: bool,
}

impl Flow {
    fn reference() -> Self {
        Self {
            has_reference: true,
            ..Self::default()
        }
    }

    fn call() -> Self {
        Self {
            has_call: true,
            ..Self::default()
        }
    }

    fn then(self, next: Self) -> Self {
        Self {
            has_reference: self.has_reference || next.has_reference,
            has_call: self.has_call || next.has_call,
            reference_before_call: self.reference_before_call
                || next.reference_before_call
                || (self.has_reference && next.has_call),
            call_before_reference: self.call_before_reference
                || next.call_before_reference
                || (self.has_call && next.has_reference),
            reference_call_reference: self.reference_call_reference
                || next.reference_call_reference
                || (self.reference_before_call && next.has_reference)
                || (self.has_reference && next.call_before_reference),
        }
    }

    fn either(self, alternative: Self) -> Self {
        Self {
            has_reference: self.has_reference || alternative.has_reference,
            has_call: self.has_call || alternative.has_call,
            reference_before_call: self.reference_before_call
                || alternative.reference_before_call,
            call_before_reference: self.call_before_reference
                || alternative.call_before_reference,
            reference_call_reference: self.reference_call_reference
                || alternative.reference_call_reference,
        }
    }
}

pub(super) fn references_span_call(
    function: &Function,
    symbols: &std::collections::HashSet<String>,
) -> bool {
    references_span_call_where(function, &|expression| {
        matches!(expression, Expression::Variable(name) if symbols.contains(name))
    })
}

pub(super) fn member_references_span_call(
    function: &Function,
    global: &str,
    offset: u32,
) -> bool {
    references_span_call_where(function, &|expression| {
        matches!(
            expression,
            Expression::Member {
                base,
                offset: member_offset,
                index_stride: None,
                ..
            } if *member_offset == offset
                && matches!(base.as_ref(), Expression::Variable(name) if name == global)
        )
    })
}

fn references_span_call_where(
    function: &Function,
    reference: &impl Fn(&Expression) -> bool,
) -> bool {
    block_flow(&function.statements, reference)
        .then(
            function
                .return_expression
                .as_ref()
                .map_or_else(Flow::default, |value| expression_flow(value, reference)),
        )
        .reference_call_reference
}

fn block_flow(
    statements: &[Statement],
    reference: &impl Fn(&Expression) -> bool,
) -> Flow {
    statements.iter().fold(Flow::default(), |flow, statement| {
        flow.then(statement_flow(statement, reference))
    })
}

fn statement_flow(
    statement: &Statement,
    reference: &impl Fn(&Expression) -> bool,
) -> Flow {
    match statement {
        Statement::Store { target, value } => {
            expression_flow(target, reference).then(expression_flow(value, reference))
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_flow(value, reference)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => expression_flow(condition, reference).then(
            block_flow(then_body, reference).either(block_flow(else_body, reference)),
        ),
        Statement::Return(value) => value
            .as_ref()
            .map_or_else(Flow::default, |value| expression_flow(value, reference)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            let alternatives = arms
                .iter()
                .map(|arm| arm_flow(&arm.body, reference))
                .chain(default.iter().map(|body| arm_flow(body, reference)))
                .fold(Flow::default(), Flow::either);
            expression_flow(scrutinee, reference).then(alternatives)
        }
        Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } => {
            let initializer = initializer
                .as_ref()
                .map_or_else(Flow::default, |value| expression_flow(value, reference));
            let condition = condition
                .as_ref()
                .map_or_else(Flow::default, |value| expression_flow(value, reference));
            let step = step
                .as_ref()
                .map_or_else(Flow::default, |value| expression_flow(value, reference));
            let body = block_flow(body, reference);
            let iteration = match kind {
                LoopKind::While | LoopKind::For => condition.then(body).then(step),
                LoopKind::DoWhile => body.then(step).then(condition),
            };
            // Two iterations are enough to expose every three-event ordering
            // that crosses a back edge (reference, call, reference).
            initializer.then(iteration).then(iteration)
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => Flow::default(),
    }
}

fn arm_flow(body: &ArmBody, reference: &impl Fn(&Expression) -> bool) -> Flow {
    match body {
        ArmBody::Return(value) => expression_flow(value, reference),
        ArmBody::Statements(statements) => block_flow(statements, reference),
    }
}

fn expression_flow(
    expression: &Expression,
    reference: &impl Fn(&Expression) -> bool,
) -> Flow {
    if reference(expression) {
        return Flow::reference();
    }
    match expression {
        Expression::AggregateLiteral(elements) => elements.iter().fold(
            Flow::default(),
            |flow, element| flow.then(expression_flow(element, reference)),
        ),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            expression_flow(left, reference).then(expression_flow(right, reference))
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => expression_flow(condition, reference).then(
            expression_flow(when_true, reference)
                .either(expression_flow(when_false, reference)),
        ),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => {
            expression_flow(operand, reference)
        }
        Expression::BitFieldRead {
            extracted, storage, ..
        }
        | Expression::Index {
            base: extracted,
            index: storage,
        } => expression_flow(extracted, reference).then(expression_flow(storage, reference)),
        Expression::Call { arguments, .. } => {
            arguments_flow(arguments, reference).then(Flow::call())
        }
        Expression::CallThrough { target, arguments } => expression_flow(target, reference)
            .then(arguments_flow(arguments, reference))
            .then(Flow::call()),
        Expression::VirtualCall {
            object, arguments, ..
        } => expression_flow(object, reference)
            .then(arguments_flow(arguments, reference))
            .then(Flow::call()),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => expression_flow(allocation, reference)
            .then(arguments_flow(arguments, reference))
            .then(Flow::call()),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => Flow::default(),
    }
}

fn arguments_flow(
    arguments: &[Expression],
    reference: &impl Fn(&Expression) -> bool,
) -> Flow {
    arguments.iter().fold(Flow::default(), |flow, argument| {
        flow.then(expression_flow(argument, reference))
    })
}
