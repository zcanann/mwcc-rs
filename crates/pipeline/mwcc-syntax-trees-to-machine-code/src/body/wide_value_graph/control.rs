//! Control-flow edges for word/pair values. Cyclic bodies give each C local a
//! stable virtual home; assignments become explicit copies, so back edges and
//! early exits use ordinary register liveness instead of recursive value aliases.
use super::*;
use mwcc_syntax_trees::LoopKind;

pub(super) fn requires_homes(statements: &[Statement], nested: bool) -> bool {
    statements
        .iter()
        .enumerate()
        .any(|(index, statement)| match statement {
            Statement::Loop { .. } | Statement::Break | Statement::Continue => true,
            Statement::Return(_) => nested || index + 1 != statements.len(),
            Statement::If {
                then_body,
                else_body,
                ..
            } => requires_homes(then_body, true) || requires_homes(else_body, true),
            _ => false,
        })
}

pub(super) fn falls_through(statements: &[Statement]) -> bool {
    for statement in statements {
        match statement {
            Statement::Return(_) | Statement::Break | Statement::Continue => return false,
            Statement::If {
                then_body,
                else_body,
                ..
            } if !falls_through(then_body) && !falls_through(else_body) => return false,
            _ => {}
        }
    }
    true
}

impl Graph<'_> {
    pub(super) fn truth(&mut self, value: Value) -> Value {
        if !wide(value.ty) {
            return value;
        }
        let result = self.fresh(Type::Int);
        self.operations.push(Operation::Binary {
            result,
            operator: BinaryOperator::NotEqual,
            left: value,
            right: Value {
                ty: value.ty,
                source: Source::Constant(0),
            },
        });
        result
    }

    fn label(&mut self) -> usize {
        let label = self.labels;
        self.labels += 1;
        label
    }

    pub(super) fn effect(&mut self, expression: &Expression) -> Option<()> {
        match expression {
            Expression::Call { name, arguments } => {
                self.call(name, arguments, false)?;
            }
            _ => {
                self.expression(expression)?;
            }
        }
        Some(())
    }

    pub(super) fn loop_body(
        &mut self,
        kind: LoopKind,
        initializer: Option<&Expression>,
        condition: Option<&Expression>,
        step: Option<&Expression>,
        body: &[Statement],
    ) -> Option<()> {
        if !self.fixed_bindings {
            return None;
        }
        if let Some(initializer) = initializer {
            self.effect(initializer)?;
        }
        let head = self.label();
        let next = self.label();
        let end = self.label();
        self.operations.push(Operation::Label(head));
        if kind != LoopKind::DoWhile {
            self.loop_condition(condition, end)?;
        }
        self.loop_targets.push((end, next));
        self.statements(body)?;
        self.loop_targets.pop();
        self.operations.push(Operation::Label(next));
        if let Some(step) = step {
            self.effect(step)?;
        }
        if kind == LoopKind::DoWhile {
            self.loop_condition(condition, end)?;
        }
        self.operations.push(Operation::Jump(head));
        self.operations.push(Operation::Label(end));
        Some(())
    }

    fn loop_condition(&mut self, condition: Option<&Expression>, end: usize) -> Option<()> {
        if let Some(expression) = condition {
            let condition = self.expression(expression)?;
            let condition = self.truth(condition);
            self.operations.push(Operation::BranchIfZero {
                condition,
                target: end,
            });
        }
        Some(())
    }
}
