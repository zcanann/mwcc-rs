//! Split non-escaping floating aggregate temporaries into scalar locals.
//!
//! This changes storage, not evaluation order: field writes remain assignments
//! at their original statement positions. Address exposure, overlapping views,
//! volatile storage, and unsupported uses leave the entire object intact.

use crate::analysis::{expression_reads_name, function_uses_name};
use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::BTreeMap;

pub(super) fn scalarize(function: &Function) -> Option<Function> {
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return None;
    }
    for local in &function.locals {
        let Type::Struct { size, .. } = local.declared_type else {
            continue;
        };
        if local.is_static
            || local.is_volatile
            || local.initializer.is_some()
            || local.array_length.is_some()
            || local.data_bytes.is_some()
        {
            continue;
        }
        let mut fields = Fields {
            local: &local.name,
            size,
            names: BTreeMap::new(),
        };
        if let Some(rewritten) = fields.rewrite(function, local) {
            return Some(rewritten);
        }
    }
    None
}

struct Fields<'a> {
    local: &'a str,
    size: u32,
    names: BTreeMap<u32, String>,
}

impl Fields<'_> {
    fn field(&mut self, expression: &Expression) -> Option<String> {
        let Expression::Member {
            base,
            offset,
            member_type: Type::Float,
            index_stride: None,
        } = expression
        else {
            return None;
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == self.local)
            || offset % 4 != 0
            || offset.checked_add(4)? > self.size
        {
            return None;
        }
        Some(
            self.names
                .entry(*offset)
                .or_insert_with(|| format!("{}$field{}", self.local, offset))
                .clone(),
        )
    }

    fn expression(&mut self, expression: &Expression) -> Option<Expression> {
        if !expression_reads_name(expression, self.local) {
            return Some(expression.clone());
        }
        if let Some(name) = self.field(expression) {
            return Some(Expression::Variable(name));
        }
        Some(match expression {
            Expression::Binary {
                operator,
                left,
                right,
            } => Expression::Binary {
                operator: *operator,
                left: Box::new(self.expression(left)?),
                right: Box::new(self.expression(right)?),
            },
            Expression::Unary { operator, operand } => Expression::Unary {
                operator: *operator,
                operand: Box::new(self.expression(operand)?),
            },
            Expression::Cast {
                target_type,
                operand,
            } => Expression::Cast {
                target_type: *target_type,
                operand: Box::new(self.expression(operand)?),
            },
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                origin,
            } => Expression::Conditional {
                condition: Box::new(self.expression(condition)?),
                when_true: Box::new(self.expression(when_true)?),
                when_false: Box::new(self.expression(when_false)?),
                origin: *origin,
            },
            Expression::Comma { left, right } => Expression::Comma {
                left: Box::new(self.expression(left)?),
                right: Box::new(self.expression(right)?),
            },
            Expression::Call { name, arguments } => Expression::Call {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|arg| self.expression(arg))
                    .collect::<Option<_>>()?,
            },
            // In particular, never turn &object.field into &scalar: another
            // observer could depend on object identity or adjacent fields.
            _ => return None,
        })
    }

    fn optional_expression(
        &mut self,
        expression: Option<&Expression>,
    ) -> Option<Option<Expression>> {
        match expression {
            None => Some(None),
            Some(value) => self.expression(value).map(Some),
        }
    }

    fn statements(&mut self, statements: &[Statement]) -> Option<Vec<Statement>> {
        statements
            .iter()
            .map(|statement| self.statement(statement))
            .collect()
    }

    fn statement(&mut self, statement: &Statement) -> Option<Statement> {
        Some(match statement {
            Statement::Store { target, value } => {
                let value = self.expression(value)?;
                if let Some(name) = self.field(target) {
                    Statement::Assign { name, value }
                } else {
                    Statement::Store {
                        target: self.expression(target)?,
                        value,
                    }
                }
            }
            Statement::Assign { name, value } if name != self.local => Statement::Assign {
                name: name.clone(),
                value: self.expression(value)?,
            },
            Statement::Expression(value) => Statement::Expression(self.expression(value)?),
            Statement::Return(value) => {
                Statement::Return(self.optional_expression(value.as_ref())?)
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => Statement::If {
                condition: self.expression(condition)?,
                then_body: self.statements(then_body)?,
                else_body: self.statements(else_body)?,
            },
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => Statement::Loop {
                kind: *kind,
                initializer: self.optional_expression(initializer.as_ref())?,
                condition: self.optional_expression(condition.as_ref())?,
                step: self.optional_expression(step.as_ref())?,
                body: self.statements(body)?,
            },
            Statement::InlineAsm(_) | Statement::Switch { .. } => return None,
            // Unsupported statement trees are checked for residual uses by
            // rewrite(), so an unhandled escape cannot partially split an object.
            other => other.clone(),
        })
    }

    fn rewrite(&mut self, function: &Function, local: &LocalDeclaration) -> Option<Function> {
        let mut rewritten = function.clone();
        rewritten.statements = self.statements(&function.statements)?;
        for guard in &mut rewritten.guards {
            guard.condition = self.expression(&guard.condition)?;
            guard.value = self.expression(&guard.value)?;
        }
        rewritten.return_expression =
            self.optional_expression(function.return_expression.as_ref())?;
        for declaration in &mut rewritten.locals {
            declaration.initializer = self.optional_expression(declaration.initializer.as_ref())?;
        }
        if self.names.is_empty() || function_uses_name(&rewritten, self.local) {
            return None;
        }
        // Names cannot collide with source or previously synthesized declarations.
        if self.names.values().any(|name| {
            function_uses_name(function, name)
                || function.locals.iter().any(|local| &local.name == name)
                || function
                    .parameters
                    .iter()
                    .any(|parameter| &parameter.name == name)
        }) {
            return None;
        }
        let index = rewritten
            .locals
            .iter()
            .position(|candidate| candidate.name == self.local)?;
        rewritten.locals.splice(
            index..=index,
            self.names.values().map(|name| LocalDeclaration {
                name: name.clone(),
                declared_type: Type::Float,
                ..local.clone()
            }),
        );
        Some(rewritten)
    }
}
