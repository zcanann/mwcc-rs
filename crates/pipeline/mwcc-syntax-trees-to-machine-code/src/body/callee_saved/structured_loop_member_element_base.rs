//! Shared object-element bases in loops with prescaled member-array offsets.
//!
//! A loop can read one embedded record and write another array with the same
//! logical element stride. After byte-offset strength reduction, MWCC forms
//! `object + byte_offset` once and addresses both members by displacement.
//! Materializing that element base before liveness gives the value its real
//! cross-call lifetime and keeps address selection independent of allocation.

#[allow(unused_imports)]
use super::*;

pub(super) const PREFIX: &str = "__mwcc_member_element_base_";

pub(super) fn materialize_member_element_bases(function: &Function) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let mut changed = false;
    let statements = function
        .statements
        .iter()
        .map(|statement| {
            rewrite_statement(
                statement,
                &mut used,
                &mut declarations,
                &mut next_name,
                &mut changed,
            )
        })
        .collect();
    changed.then(|| {
        let mut rewritten = function.clone();
        rewritten.locals.extend(declarations);
        rewritten.statements = statements;
        rewritten
    })
}

fn rewrite_statement(
    statement: &Statement,
    used: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
    changed: &mut bool,
) -> Statement {
    if let Statement::Loop {
        kind,
        initializer,
        condition,
        step,
        body,
    } = statement
    {
        if let Some(plan) = Plan::recognize(initializer.as_ref(), step.as_ref(), body) {
            let cursor = fresh_name(used, next_name);
            declarations.push(LocalDeclaration {
                declared_type: Type::Pointer(Pointee::UnsignedChar),
                name: cursor.clone(),
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
            let mut rewritten_body = Vec::with_capacity(body.len() + 1);
            rewritten_body.push(Statement::Assign {
                name: cursor.clone(),
                value: plan.cursor_value(),
            });
            rewritten_body.extend(
                body.iter()
                    .map(|statement| plan.rewrite_statement(statement, &cursor)),
            );
            *changed = true;
            return Statement::Loop {
                kind: *kind,
                initializer: initializer.clone(),
                condition: condition.clone(),
                step: step.clone(),
                body: rewritten_body,
            };
        }
        return Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body: body
                .iter()
                .map(|inner| {
                    rewrite_statement(inner, used, declarations, next_name, changed)
                })
                .collect(),
        };
    }
    match statement {
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: condition.clone(),
            then_body: then_body
                .iter()
                .map(|inner| {
                    rewrite_statement(inner, used, declarations, next_name, changed)
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|inner| {
                    rewrite_statement(inner, used, declarations, next_name, changed)
                })
                .collect(),
        },
        _ => statement.clone(),
    }
}

#[derive(Clone)]
struct Plan {
    owner: Expression,
    logical_index: String,
    offset: String,
    stride: u32,
}

impl Plan {
    fn recognize(
        initializer: Option<&Expression>,
        step: Option<&Expression>,
        body: &[Statement],
    ) -> Option<Self> {
        if !body.iter().any(crate::analysis::statement_has_call) {
            return None;
        }
        let induction = InductionPair::parse(initializer?, step?)?;
        let mut groups: Vec<(Self, usize, Vec<u32>)> = Vec::new();
        for statement in body {
            super::structured_expression_visit::visit_statement(statement, &mut |expression| {
                let Some(access) = Access::parse(expression) else {
                    return;
                };
                if !induction.accepts(access.index, access.stride) {
                    return;
                }
                if let Some((_, count, offsets)) = groups.iter_mut().find(|(plan, _, _)| {
                    plan.stride == access.stride
                        && crate::analysis::structurally_equal(&plan.owner, access.owner)
                }) {
                    *count += 1;
                    if !offsets.contains(&access.member_offset) {
                        offsets.push(access.member_offset);
                    }
                } else {
                    groups.push((
                        Self {
                            owner: access.owner.clone(),
                            logical_index: induction.logical.to_owned(),
                            offset: induction.offset.to_owned(),
                            stride: access.stride,
                        },
                        1,
                        vec![access.member_offset],
                    ));
                }
            });
        }
        let mut eligible = groups
            .into_iter()
            .filter(|(_, count, offsets)| *count >= 3 && offsets.len() >= 2);
        let plan = eligible.next()?.0;
        eligible.next().is_none().then_some(plan)
    }

    fn cursor_value(&self) -> Expression {
        Expression::AddressOf {
            operand: Box::new(Expression::Index {
                base: Box::new(Expression::MemberAddress {
                    base: Box::new(self.owner.clone()),
                    offset: 0,
                    element: Pointee::UnsignedChar,
                    index_stride: None,
                }),
                index: Box::new(Expression::Variable(self.offset.clone())),
            }),
        }
    }

    fn rewrite_statement(&self, statement: &Statement, cursor: &str) -> Statement {
        super::structured_expression_visit::rewrite_statement(statement, &mut |expression| {
            let access = Access::parse(expression)?;
            ((access.index == self.logical_index.as_str()
                || access.index == self.offset.as_str())
                && access.stride == self.stride
                && crate::analysis::structurally_equal(&self.owner, access.owner))
            .then(|| access.as_member(cursor))
        })
    }
}

struct InductionPair<'a> {
    logical: &'a str,
    offset: &'a str,
    logical_step: i64,
    offset_step: i64,
}

impl<'a> InductionPair<'a> {
    fn parse(initializer: &'a Expression, step: &'a Expression) -> Option<Self> {
        let Expression::Comma {
            left: logical_initializer,
            right: offset_initializer,
        } = initializer
        else {
            return None;
        };
        let logical = zero_initializer(logical_initializer)?;
        let offset = zero_initializer(offset_initializer)?;
        if !offset.starts_with(crate::analysis::PRESCALED_MEMBER_ARRAY_INDEX_PREFIX) {
            return None;
        }
        let Expression::Comma {
            left: logical_increment,
            right: offset_increment,
        } = step
        else {
            return None;
        };
        let (stepped_logical, logical_step) = counted_step(logical_increment)?;
        let (stepped_offset, offset_step) = counted_step(offset_increment)?;
        (stepped_logical == logical
            && stepped_offset == offset
            && logical_step > 0
            && offset_step > 0)
            .then_some(Self {
                logical,
                offset,
                logical_step,
                offset_step,
            })
    }

    fn accepts(&self, index: &str, stride: u32) -> bool {
        (index == self.logical || index == self.offset)
            && self.logical_step.checked_mul(i64::from(stride)) == Some(self.offset_step)
    }
}

struct Access<'a> {
    owner: &'a Expression,
    index: &'a str,
    stride: u32,
    member_offset: u32,
    member_type: Type,
}

impl<'a> Access<'a> {
    fn parse(expression: &'a Expression) -> Option<Self> {
        let Expression::Index { base, index } = expression else {
            return None;
        };
        let Expression::Variable(index) = index.as_ref() else {
            return None;
        };
        let (owner, member_offset, member_type, stride) = match base.as_ref() {
            Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size, align },
                index_stride: None,
            } if *size != 0 => (
                base.as_ref(),
                *offset,
                Type::Struct {
                    size: *size,
                    align: *align,
                },
                *size,
            ),
            Expression::MemberAddress {
                base,
                offset,
                element,
                index_stride: None,
            } => (
                base.as_ref(),
                *offset,
                element.element(),
                u32::from(element.size()),
            ),
            _ => return None,
        };
        Some(Self {
            owner,
            index,
            stride,
            member_offset,
            member_type,
        })
    }

    fn as_member(&self, cursor: &str) -> Expression {
        let member = Expression::Member {
            base: Box::new(Expression::Variable(cursor.to_owned())),
            offset: self.member_offset,
            member_type: self.member_type,
            index_stride: None,
        };
        if matches!(self.member_type, Type::Struct { .. }) {
            // Keep narrow whole-object copies in their indexed aggregate form.
            // That form owns the unsigned byte/halfword representation load;
            // a plain aggregate member would enter the word-copy path.
            Expression::Index {
                base: Box::new(member),
                index: Box::new(Expression::IntegerLiteral(0)),
            }
        } else {
            member
        }
    }
}

fn zero_initializer(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(name) = target.as_ref() else {
        return None;
    };
    (crate::analysis::constant_value(value) == Some(0)).then_some(name)
}

fn counted_step(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(name) = target.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    let amount = crate::analysis::constant_value(right)?;
    matches!(left.as_ref(), Expression::Variable(read) if read == name)
        .then_some((name, amount))
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next: &mut usize) -> String {
    loop {
        let name = format!("{PREFIX}{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

#[cfg(test)]
#[path = "structured_loop_member_element_base_tests.rs"]
mod tests;
