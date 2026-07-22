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
            .filter(|name| read_after_possible_call(&function.statements, name, false).0)
            .collect();
        if survivors.is_empty() {
            return Ok(false);
        }

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
            let temporary = self.fresh_virtual_general();
            self.evaluate(
                local.initializer.as_ref().expect("eligibility checked"),
                local.declared_type,
                temporary,
            )?;
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
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

        self.emit_structured_statements(&function.statements, function)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_structured_statements(
        &mut self,
        statements: &[Statement],
        function: &Function,
    ) -> Compilation<()> {
        for (statement_index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if else_body.is_empty() => {
                    let (options, condition_bit) =
                        self.emit_condition_test(condition)
                            .map_err(|mut diagnostic| {
                                diagnostic.message.push_str(&format!(
                                    " (in structured if condition {statement_index})"
                                ));
                                diagnostic
                            })?;
                    let branch = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    self.emit_structured_statements(then_body, function)
                        .map_err(|mut diagnostic| {
                            diagnostic.message.push_str(&format!(
                                " (inside structured if statement {statement_index})"
                            ));
                            diagnostic
                        })?;
                    let target = self.output.instructions.len();
                    if let Instruction::BranchConditionalForward {
                        target: branch_target,
                        ..
                    } = &mut self.output.instructions[branch]
                    {
                        *branch_target = target;
                    }
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
        | Statement::Expression(Expression::VirtualCall { .. }) => true,
        Statement::Assign { name, .. } => function.locals.iter().any(|local| &local.name == name),
        Statement::If {
            then_body,
            else_body,
            ..
        } => else_body.is_empty() && supports_statements(then_body, function),
        _ => false,
    })
}

/// `(name is read after a possible call, a call may have occurred afterwards)`.
fn read_after_possible_call(
    statements: &[Statement],
    name: &str,
    mut prior_call: bool,
) -> (bool, bool) {
    let mut read_after = false;
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                read_after |= prior_call && expression_reads_name(condition, name);
                let (then_read, then_call) = read_after_possible_call(then_body, name, prior_call);
                let (else_read, else_call) = read_after_possible_call(else_body, name, prior_call);
                read_after |= then_read || else_read;
                prior_call = prior_call || then_call || else_call;
            }
            Statement::Store { target, value } => {
                read_after |= prior_call
                    && (expression_reads_name(target, name) || expression_reads_name(value, name));
                prior_call |= statement_has_call(statement);
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => {
                read_after |= prior_call && expression_reads_name(value, name);
                prior_call |= statement_has_call(statement);
            }
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_)
            | Statement::Switch { .. }
            | Statement::Loop { .. } => {
                prior_call |= statement_has_call(statement);
            }
        }
    }
    (read_after, prior_call)
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
        assert!(read_after_possible_call(&statements, "pointer", false).0);
        assert!(!read_after_possible_call(&statements, "condition", false).0);
    }
}
