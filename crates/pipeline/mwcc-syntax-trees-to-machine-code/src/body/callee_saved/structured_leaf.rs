//! Frame-free entry point for the shared structured-control-flow lowerer.
//!
//! Structured lowering originally lived behind the callee-saved frame owner,
//! even though its branch graph is equally useful for leaf functions. This
//! adapter owns no prologue or allocation policy: it admits only frame-free
//! bodies and delegates their nested regions to the common emitter.

#[allow(unused_imports)]
use super::*;
use super::structured_early_return_schedule::resolve_leaf_structured_returns;
use super::structured::structured_hidden_label_count;

impl Generator {
    pub(crate) fn try_leaf_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let structured_statements = leaf_structured_statements(function);
        if function_makes_call(function)
            || !self.frame_slots.is_empty()
            || !leaf_return_shape_is_supported(function)
            || !requires_structured_branch_graph(&structured_statements)
            || !supports_leaf_structured_statements(&structured_statements)
            || function.locals.iter().any(|local| {
                local.is_static
                    || local.array_length.is_some()
                    || !matches!(
                        class_of(local.declared_type),
                        Ok(ValueClass::General | ValueClass::Float)
                    )
            })
        {
            return Ok(false);
        }

        if let Some(work_homes) =
            super::structured_branch_work_homes::StructuredBranchWorkHomes::plan(self, function)
        {
            self.structured_branch_float_work_home = Some(work_homes.float);
            self.structured_branch_constant_address_home = Some(work_homes.constant_address);
        }

        for local in &function.locals {
            let class = class_of(local.declared_type).expect("eligibility checked");
            let home = match class {
                ValueClass::General => self.fresh_virtual_general_preferring(4),
                ValueClass::Float => self.fresh_virtual_float_preferring(1),
            };
            if let Some(initializer) = &local.initializer {
                self.evaluate(initializer, local.declared_type, home)?;
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
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

        let mut return_branches = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut pending_gotos = Vec::new();
        self.emit_structured_statements(
            &structured_statements,
            function,
            &[],
            false,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut None,
        )?;
        self.retain_guarded_nested_member_base();
        self.reuse_guarded_narrow_member_update();
        self.schedule_volatile_bitset_hint_tail();
        self.schedule_leaf_global_store_compare();
        debug_assert!(pending_gotos.is_empty());
        if let Some(return_expression) = &function.return_expression {
            let result = match function.return_type {
                Type::Float | Type::Double => Eabi::float_result().number,
                _ => Eabi::general_result().number,
            };
            self.evaluate(return_expression, function.return_type, result)?;
        }
        let epilogue = self.output.instructions.len();
        resolve_leaf_structured_returns(&mut self.output.instructions, epilogue);
        self.output.anonymous_label_bump += structured_hidden_label_count(&structured_statements);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Fill a global store's issue slot with the first instruction of the
    /// following comparison when both consume the same register. The compare
    /// materialization is independent of the store and MWCC schedules it first.
    fn schedule_leaf_global_store_compare(&mut self) {
        let Some((from, to)) = leaf_global_store_compare_move(&self.output.instructions) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, from, to);
        // The moved compare now begins the continuation that previously began
        // at the store. Incoming control-flow edges must include that hoisted
        // instruction rather than preserving the store as their destination.
        for instruction in &mut self.output.instructions[..to] {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == to + 1 =>
                {
                    *target = to;
                }
                _ => {}
            }
        }
    }
}

fn leaf_global_store_compare_move(instructions: &[Instruction]) -> Option<(usize, usize)> {
    instructions.windows(4).enumerate().find_map(|(index, window)| {
        matches!(
            window,
            [
                Instruction::StoreWord {
                    s: stored,
                    a: 0,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: compared,
                    ..
                },
                Instruction::CompareLogicalWordImmediate { a: 0, .. },
                Instruction::BranchConditionalForward { .. },
            ] if stored == compared
        )
        .then_some((index + 1, index))
    })
}

/// Put parser-extracted terminal guards back into source CFG form for the
/// structured emitter. Guards are semantically `if (condition) return value;`
/// statements immediately before the function's final return expression.
fn leaf_structured_statements(function: &Function) -> Vec<Statement> {
    let mut statements = function.statements.clone();
    statements.extend(function.guards.iter().map(|guard| Statement::If {
        condition: guard.condition.clone(),
        then_body: vec![Statement::Return(Some(guard.value.clone()))],
        else_body: Vec::new(),
    }));
    statements
}

fn requires_structured_branch_graph(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.len() > 1
                || !else_body.is_empty()
                || then_body.iter().any(|inner| matches!(inner, Statement::If { .. }))
                || requires_structured_branch_graph(then_body)
                || requires_structured_branch_graph(else_body)
        }
        _ => false,
    })
}

fn supports_leaf_structured_statements(statements: &[Statement]) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { .. } | Statement::Store { .. } | Statement::Return(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            supports_leaf_structured_statements(then_body)
                && supports_leaf_structured_statements(else_body)
        }
        _ => false,
    })
}

fn leaf_return_shape_is_supported(function: &Function) -> bool {
    (function.return_type == Type::Void && function.return_expression.is_none())
        || (matches!(
            function.return_type,
            Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. }
                | Type::Float
                | Type::Double
        ) && (function.return_expression.is_some()
            || !leaf_statements_fall_through(&function.statements)))
}

/// Whether execution can reach the implicit tail after this structured list.
/// Eligibility excludes loops and gotos, so returns and complete if/else
/// diamonds are the only terminating edges that need modeling here.
fn leaf_statements_fall_through(statements: &[Statement]) -> bool {
    for statement in statements {
        match statement {
            Statement::Return(_) => return false,
            Statement::If {
                then_body,
                else_body,
                ..
            } if !else_body.is_empty()
                && !leaf_statements_fall_through(then_body)
                && !leaf_statements_fall_through(else_body) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_statement_guard_uses_the_structured_branch_graph() {
        let statements = vec![Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Variable("first".into()),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Store {
                    target: Expression::Variable("second".into()),
                    value: Expression::IntegerLiteral(2),
                },
            ],
            else_body: Vec::new(),
        }];
        assert!(requires_structured_branch_graph(&statements));
    }

    #[test]
    fn appends_parser_guards_as_structured_early_returns() {
        let mut function = Function {
            return_type: Type::Int,
            name: "classify".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(3)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        function.statements.push(Statement::Store {
            target: Expression::Variable("seen".into()),
            value: Expression::IntegerLiteral(1),
        });
        function.guards.push(mwcc_syntax_trees::GuardedReturn {
            condition: Expression::Variable("matched".into()),
            value: Expression::IntegerLiteral(2),
        });

        let statements = leaf_structured_statements(&function);
        assert!(matches!(
            statements.as_slice(),
            [
                Statement::Store { .. },
                Statement::If {
                    then_body,
                    else_body,
                    ..
                }
            ] if matches!(then_body.as_slice(), [Statement::Return(Some(Expression::IntegerLiteral(2)))])
                && else_body.is_empty()
        ));
    }

    #[test]
    fn moves_a_following_compare_materialization_before_a_global_store() {
        let instructions = vec![
            Instruction::StoreWord {
                s: 4,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 4,
                immediate: -3,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0x1100,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
        ];

        assert_eq!(leaf_global_store_compare_move(&instructions), Some((1, 0)));
    }
}
