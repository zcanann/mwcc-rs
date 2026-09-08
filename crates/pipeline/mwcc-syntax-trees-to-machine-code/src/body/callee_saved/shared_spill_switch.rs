//! GC/1.1p1 O0 parameter images around a scalar switch and a joined call.
//!
//! Single-use parameters share SP+8 in declaration order. A repeatedly read
//! input instead occupies r30, whose saved image is overwritten by that same
//! spill. Keep these source images separate from ordinary allocator spill slots.

use super::structured_expression_visit::rewrite_expression;
use super::*;
use mwcc_syntax_trees::ArmBody;

fn word(ty: Type) -> bool {
    matches!(ty, Type::Int | Type::UnsignedInt)
}

/// Count input reads while admitting only scalar expressions whose complete
/// effects and local initialization are known to this source-image plan.
fn input_reads(value: &Expression, input: &str, local: &str, initialized: bool) -> Option<usize> {
    match value {
        Expression::IntegerLiteral(_) => Some(0),
        Expression::Variable(name) if name == input => Some(1),
        Expression::Variable(name) if name == local && initialized => Some(0),
        Expression::Binary {
            operator:
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor,
            left,
            right,
        } => Some(
            input_reads(left, input, local, initialized)?
                + input_reads(right, input, local, initialized)?,
        ),
        Expression::Cast {
            target_type,
            operand,
        } if word(*target_type) => input_reads(operand, input, local, initialized),
        _ => None,
    }
}

fn arm_reads(body: &ArmBody, input: &str, local: &str, mut initialized: bool) -> Option<usize> {
    let ArmBody::Statements(statements) = body else {
        return None;
    };
    let mut reads = 0;
    for statement in statements {
        match statement {
            Statement::Assign { name, value } if name == local => {
                reads += input_reads(value, input, local, initialized)?;
                initialized = true;
            }
            Statement::Expression(Expression::Variable(name)) if name == local && initialized => {}
            _ => return None,
        }
    }
    initialized.then_some(reads)
}

impl Generator {
    pub(crate) fn try_shared_spill_switch(&mut self, function: &Function) -> Compilation<bool> {
        let [selector, input] = function.parameters.as_slice() else {
            return Ok(false);
        };
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        let [Statement::Switch {
            scrutinee,
            arms,
            default,
        }, Statement::Expression(Expression::Call {
            name: callee,
            arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [argument] = arguments.as_slice() else {
            return Ok(false);
        };
        if !self.behavior.unoptimized_shared_parameter_spills
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || !function.inline_asm_blocks.is_empty()
            || function.asm_body.is_some()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !word(selector.parameter_type)
            || !word(input.parameter_type)
            || !word(local.declared_type)
            || local.is_static
            || local.is_volatile
            || local.array_length.is_some()
            || !matches!(scrutinee, Expression::Variable(name) if name == &selector.name)
            || self.skipped_inline_names.contains(callee)
            || self.globals.contains_key(callee)
            || self.call_return_types.get(callee) != Some(&Type::Void)
            || !matches!(self.call_parameter_types.get(callee).map(Vec::as_slice), Some([ty]) if word(*ty))
            || arms.is_empty()
            || arms.len() > 6
            || arms.iter().any(|arm| arm.falls_through)
        {
            return Ok(false);
        }
        let low = arms.iter().map(|arm| arm.value).min().unwrap();
        let high = arms.iter().map(|arm| arm.value).max().unwrap();
        if low < i16::MIN as i64 || high >= i16::MAX as i64 || high - low >= 6 {
            return Ok(false);
        }
        let initialized = local.initializer.is_some();
        let Some(mut reads) = local.initializer.as_ref().map_or(Some(0), |value| {
            input_reads(value, &input.name, &local.name, false)
        }) else {
            return Ok(false);
        };
        for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
            let Some(count) = arm_reads(body, &input.name, &local.name, initialized) else {
                return Ok(false);
            };
            reads += count;
        }
        if default.is_none() && !initialized {
            return Ok(false);
        }
        let Some(count) = input_reads(argument, &input.name, &local.name, true) else {
            return Ok(false);
        };
        reads += count;
        if reads == 0 || !expression_reads_name(argument, &local.name) {
            return Ok(false);
        }
        let retained_input = reads > 1;
        let stack = (0..)
            .map(|n| format!("__mwcc_switch_spill_stack_{n}"))
            .find(|name| {
                !self.locations.contains_key(name)
                    && function.parameters.iter().all(|p| p.name != *name)
                    && function.locals.iter().all(|p| p.name != *name)
            })
            .unwrap();
        let rewrite = |value: &Expression| {
            rewrite_expression(value, &mut |value| match value {
                Expression::Variable(name)
                    if name == &selector.name || (!retained_input && name == &input.name) =>
                {
                    Some(Expression::Member {
                        base: Box::new(Expression::Variable(stack.clone())),
                        offset: 8,
                        member_type: if name == &selector.name {
                            selector.parameter_type
                        } else {
                            input.parameter_type
                        },
                        index_stride: None,
                    })
                }
                _ => None,
            })
        };
        self.emit_linkage_first_nonleaf_prologue(if retained_input { &[31, 30] } else { &[31] });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(if retained_input {
            Instruction::move_register(30, 4)
        } else {
            Instruction::StoreWord {
                s: 4,
                a: 1,
                offset: 8,
            }
        });
        self.locations.remove(&selector.name);
        if retained_input {
            self.locations
                .get_mut(&input.name)
                .expect("source parameter has an entry home")
                .register = 30;
        } else {
            self.locations.remove(&input.name);
        }
        self.locations.insert(
            stack.clone(),
            Location {
                class: ValueClass::General,
                register: 1,
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(1),
            },
        );
        self.locations.insert(
            local.name.clone(),
            Location {
                class: ValueClass::General,
                register: 31,
                signed: local.declared_type == Type::Int,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        if let Some(value) = &local.initializer {
            self.evaluate_general(&rewrite(value), 31)?;
        }
        self.emit_joined_statement_switch_with(
            &rewrite(scrutinee),
            arms,
            default.as_ref(),
            |generator, statement| {
                match statement {
                    Statement::Assign { value, .. } => {
                        generator.evaluate_general(&rewrite(value), 31)
                    }
                    _ => Ok(()), // Validated bare hints produce no instructions.
                }
            },
        )?;
        self.evaluate_general(&rewrite(argument), 3)?;
        self.emit_call(callee, &[], None, false)?;
        self.locations.remove(&stack);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branch_reads_require_a_dominating_local_assignment() {
        let copy = Statement::Assign {
            name: "row".into(),
            value: Expression::Variable("input".into()),
        };
        let add = Statement::Assign {
            name: "row".into(),
            value: Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("row".into())),
                right: Box::new(Expression::Variable("input".into())),
            },
        };
        assert_eq!(
            arm_reads(
                &ArmBody::Statements(vec![add.clone()]),
                "input",
                "row",
                false
            ),
            None
        );
        assert_eq!(
            arm_reads(&ArmBody::Statements(vec![copy, add]), "input", "row", false),
            Some(2)
        );
        assert_eq!(
            arm_reads(&ArmBody::Statements(vec![]), "input", "row", false),
            None
        );
        assert_eq!(
            arm_reads(&ArmBody::Statements(vec![]), "input", "row", true),
            Some(0)
        );
    }

    #[test]
    fn source_images_do_not_admit_address_taken_or_effectful_values() {
        let variable = Expression::Variable("input".into());
        assert_eq!(input_reads(&variable, "input", "row", true), Some(1));
        assert_eq!(
            input_reads(
                &Expression::AddressOf {
                    operand: Box::new(variable.clone())
                },
                "input",
                "row",
                true
            ),
            None
        );
        assert_eq!(
            input_reads(
                &Expression::Call {
                    name: "helper".into(),
                    arguments: vec![variable]
                },
                "input",
                "row",
                true
            ),
            None
        );
        assert_eq!(
            input_reads(
                &Expression::Variable("selector".into()),
                "input",
                "row",
                true
            ),
            None
        );
    }
}
