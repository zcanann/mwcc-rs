//! Structured control flow whose register values survive conditional calls.
//!
//! This is the conservative bridge between semantic statement lowering and the
//! virtual-register allocator.  It owns a complete function only when every
//! statement is representable by the shared store/call emitter plus forward
//! `if` branches; unsupported control flow declines before emitting anything.

#[allow(unused_imports)]
use super::*;
use super::structured_locals::plan_ephemeral_locals;

impl Generator {
    /// Lower a void structured body after assigning every value that can be read
    /// following a (possibly conditional) call to a virtual callee-saved home.
    pub(crate) fn try_callee_saved_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !supports_statements(&function.statements, function)
        {
            return Ok(false);
        }

        let candidates: Vec<&str> = function
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .chain(
                function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str()),
            )
            .collect();
        let survivors: std::collections::HashSet<&str> = candidates
            .into_iter()
            .filter(|name| {
                read_after_possible_call(&function.statements, name, false).read_after_call
            })
            .collect();
        // Local lifetimes rank ahead of incoming parameters. Within the incoming
        // set, MWCC assigns the last parameter the highest home (r31 downward).
        let saved_locals: Vec<&LocalDeclaration> = function
            .locals
            .iter()
            .filter(|local| survivors.contains(local.name.as_str()))
            .collect();
        if saved_locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || local.initializer.is_none()
                || class_of(local.declared_type).ok() != Some(ValueClass::General)
        }) {
            return Ok(false);
        }
        let saved_parameters: Vec<_> = function
            .parameters
            .iter()
            .rev()
            .filter(|parameter| survivors.contains(parameter.name.as_str()))
            .collect();
        if saved_parameters.iter().any(|parameter| {
            self.locations
                .get(&parameter.name)
                .is_none_or(|location| location.class != ValueClass::General)
        }) {
            return Ok(false);
        }
        let Some(ephemeral_locals) = plan_ephemeral_locals(function, &survivors) else {
            return Ok(false);
        };

        let count = saved_locals.len() + saved_parameters.len();
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTable;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -plan.frame_size,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: plan.frame_size + 4,
            },
        ]);

        let mut home_index = 0;
        for local in saved_locals {
            let home = homes[home_index];
            home_index += 1;
            let slot = home_index as i16;
            self.output.instructions.push(Instruction::StoreWord {
                s: home,
                a: 1,
                offset: plan.frame_size - 4 * slot,
            });
            self.evaluate(
                local.initializer.as_ref().expect("eligibility checked"),
                local.declared_type,
                home,
            )?;
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        let mut saved_parameter_homes = Vec::new();
        for parameter in saved_parameters {
            let home = homes[home_index];
            home_index += 1;
            let incoming = self
                .locations
                .get(&parameter.name)
                .expect("eligibility checked")
                .register;
            let slot = home_index as i16;
            self.output.instructions.push(Instruction::StoreWord {
                s: home,
                a: 1,
                offset: plan.frame_size - 4 * slot,
            });
            self.output
                .instructions
                .push(Instruction::move_register(home, incoming));
            saved_parameter_homes.push((parameter.name.clone(), home));
        }
        // Initializers are evaluated at declaration time, while an incoming
        // parameter still has its entry-register alias. MWCC can preserve that
        // alias after copying the value to a saved home (`mr r31,r3; lwz ...,r3`)
        // and switches subsequent body uses to the home only after declarations.
        for local in ephemeral_locals {
            let class = class_of(local.declared_type).expect("eligibility checked");
            let temporary = match class {
                ValueClass::General => self.fresh_virtual_general(),
                ValueClass::Float => self.fresh_virtual_float(),
            };
            if let Some(initializer) = &local.initializer {
                self.evaluate(initializer, local.declared_type, temporary)?;
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
                    register: temporary,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }
        for (name, home) in saved_parameter_homes {
            self.locations
                .get_mut(&name)
                .expect("eligibility checked")
                .register = home;
        }

        let mut return_branches = Vec::new();
        self.emit_structured_statements(
            &function.statements,
            function,
            &mut return_branches,
        )?;
        let epilogue = self.output.instructions.len();
        for branch in return_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        // Each source-level `if` creates a pair of optimizer labels even when
        // both collapse to direct instruction offsets. Build 163 exposes those
        // otherwise-hidden labels through the later unwind-symbol ordinal.
        self.output.anonymous_label_bump += structured_hidden_label_count(&function.statements);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_structured_statements(
        &mut self,
        statements: &[Statement],
        function: &Function,
        return_branches: &mut Vec<usize>,
    ) -> Compilation<()> {
        for (statement_index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if else_body.is_empty() => {
                    let terms = logical_and_terms(condition);
                    let mut branches = Vec::with_capacity(terms.len());
                    for term in terms {
                        let (options, condition_bit) = self
                            .emit_condition_test(term)
                            .map_err(|mut diagnostic| {
                                diagnostic.message.push_str(&format!(
                                    " (in structured if condition {statement_index})"
                                ));
                                diagnostic
                            })?;
                        branches.push(self.output.instructions.len());
                        self.output
                            .instructions
                            .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    }
                    self.emit_structured_statements(then_body, function, return_branches)
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (inside structured if statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                    let target = self.output.instructions.len();
                    for branch in branches {
                        if let Instruction::BranchConditionalForward {
                            target: branch_target,
                            ..
                        } = &mut self.output.instructions[branch]
                        {
                            *branch_target = target;
                        }
                    }
                }
                Statement::Return(None) => {
                    return_branches.push(self.output.instructions.len());
                    self.output
                        .instructions
                        .push(Instruction::Branch { target: 0 });
                }
                Statement::Assign { name, value } => {
                    let local = function
                        .locals
                        .iter()
                        .find(|local| &local.name == name)
                        .expect("eligibility checked");
                    let destination = self
                        .locations
                        .get(name)
                        .ok_or_else(|| {
                            Diagnostic::error("structured assignment has no register home")
                        })?
                        .register;
                    self.evaluate(value, local.declared_type, destination)
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (in structured assignment statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                }
                _ => self.emit_statement(statement).map_err(|mut diagnostic| {
                    diagnostic.message.push_str(&format!(
                        " (in structured body statement {statement_index})"
                    ));
                    diagnostic
                })?,
            }
        }
        Ok(())
    }
}

fn supports_statements(statements: &[Statement], function: &Function) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Store { .. }
        | Statement::Expression(Expression::Call { .. })
        | Statement::Expression(Expression::VirtualCall { .. })
        | Statement::Return(None) => true,
        Statement::Assign { name, .. } => function.locals.iter().any(|local| &local.name == name),
        Statement::If {
            then_body,
            else_body,
            ..
        } => else_body.is_empty() && supports_statements(then_body, function),
        _ => false,
    })
}

fn structured_hidden_label_count(statements: &[Statement]) -> u32 {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                2 + logical_and_count(condition)
                    + structured_hidden_label_count(then_body)
                    + structured_hidden_label_count(else_body)
            }
            _ => 0,
        })
        .sum()
}

fn logical_and_count(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } => 1 + logical_and_count(left) + logical_and_count(right),
        _ => 0,
    }
}

fn logical_and_terms(expression: &Expression) -> Vec<&Expression> {
    let mut terms = Vec::new();
    fn collect<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
        if let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } = expression
        {
            collect(left, terms);
            collect(right, terms);
        } else {
            terms.push(expression);
        }
    }
    collect(expression, &mut terms);
    terms
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Flow {
    read_after_call: bool,
    call_on_fallthrough: bool,
    falls_through: bool,
}

/// Path-sensitive call liveness for one structured statement sequence. A call
/// in an arm that returns does not contaminate the continuation, while a call in
/// the condition reaches either arm and can make their reads require a saved
/// home.
fn read_after_possible_call(
    statements: &[Statement],
    name: &str,
    mut prior_call: bool,
) -> Flow {
    let mut read_after = false;
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                read_after |= prior_call && expression_reads_name(condition, name);
                let branch_entry_call = prior_call || expression_has_call(condition);
                let then_flow = read_after_possible_call(then_body, name, branch_entry_call);
                let else_flow = read_after_possible_call(else_body, name, branch_entry_call);
                read_after |= then_flow.read_after_call || else_flow.read_after_call;
                let then_reaches = then_flow.falls_through.then_some(then_flow.call_on_fallthrough);
                let else_reaches = else_flow.falls_through.then_some(else_flow.call_on_fallthrough);
                match (then_reaches, else_reaches) {
                    (None, None) => {
                        return Flow {
                            read_after_call: read_after,
                            call_on_fallthrough: false,
                            falls_through: false,
                        };
                    }
                    (then_call, else_call) => {
                        prior_call = then_call.unwrap_or(false) || else_call.unwrap_or(false);
                    }
                }
            }
            Statement::Store { target, value } => {
                read_after |= prior_call
                    && (expression_reads_name(target, name) || expression_reads_name(value, name));
                prior_call |= statement_has_call(statement);
            }
            Statement::Assign { value, .. } | Statement::Expression(value) => {
                read_after |= prior_call && expression_reads_name(value, name);
                prior_call |= statement_has_call(statement);
            }
            Statement::Return(expression) => {
                read_after |= prior_call
                    && expression
                        .as_ref()
                        .is_some_and(|value| expression_reads_name(value, name));
                return Flow {
                    read_after_call: read_after,
                    call_on_fallthrough: false,
                    falls_through: false,
                };
            }
            Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_)
            | Statement::Switch { .. }
            | Statement::Loop { .. } => {
                prior_call |= statement_has_call(statement);
            }
        }
    }
    Flow {
        read_after_call: read_after,
        call_on_fallthrough: prior_call,
        falls_through: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditional_calls_make_later_reads_survive() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "grow".into(),
                    arguments: vec![],
                })],
                else_body: vec![],
            },
            Statement::Store {
                target: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("pointer".into())),
                },
                value: Expression::IntegerLiteral(1),
            },
        ];
        assert!(read_after_possible_call(&statements, "pointer", false).read_after_call);
        assert!(!read_after_possible_call(&statements, "condition", false).read_after_call);
    }

    #[test]
    fn a_calling_arm_that_returns_does_not_reach_the_continuation() {
        let statements = vec![
            Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![
                    Statement::Expression(Expression::Call {
                        name: "act".into(),
                        arguments: vec![],
                    }),
                    Statement::Return(None),
                ],
                else_body: vec![],
            },
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!read_after_possible_call(&statements, "value", false).read_after_call);
    }

    #[test]
    fn a_condition_call_makes_reads_in_its_arm_live_across_the_call() {
        let statements = vec![Statement::If {
            condition: Expression::Call {
                name: "test".into(),
                arguments: vec![],
            },
            then_body: vec![Statement::Expression(Expression::Variable("value".into()))],
            else_body: vec![],
        }];
        assert!(read_after_possible_call(&statements, "value", false).read_after_call);
    }
}
