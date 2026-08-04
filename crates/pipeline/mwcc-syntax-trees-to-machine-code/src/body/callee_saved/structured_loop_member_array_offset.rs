//! Strength reduction for embedded aggregate arrays in counted loops.
//!
//! When a loop needs both its logical index and `object->records[index]`, MWCC
//! can carry the latter's byte offset in a second induction variable. Making
//! that value explicit before liveness planning gives it an independent saved
//! home and lets aggregate-copy lowering consume it without scaling again.

use super::*;

pub(super) fn strength_reduce_member_array_offsets(function: &Function) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_name = 0usize;
    let mut declarations = Vec::new();
    let mut changed = false;
    let statements = function
        .statements
        .iter()
        .map(|statement| {
            reduce_statement(
                statement,
                &mut used,
                &mut next_name,
                &mut declarations,
                &mut changed,
            )
        })
        .collect();

    changed.then(|| {
        let mut reduced = function.clone();
        reduced.locals.extend(declarations);
        reduced.statements = statements;
        reduced
    })
}

fn reduce_statement(
    statement: &Statement,
    used: &mut std::collections::HashSet<String>,
    next_name: &mut usize,
    declarations: &mut Vec<LocalDeclaration>,
    changed: &mut bool,
) -> Statement {
    if let Some(plan) = Plan::recognize(statement) {
        let offset = fresh_name(used, next_name);
        declarations.push(LocalDeclaration {
            declared_type: Type::UnsignedInt,
            name: offset.clone(),
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
        *changed = true;
        return plan.rewrite(statement, &offset);
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
                    reduce_statement(inner, used, next_name, declarations, changed)
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|inner| {
                    reduce_statement(inner, used, next_name, declarations, changed)
                })
                .collect(),
        },
        Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } => Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body: body
                .iter()
                .map(|inner| {
                    reduce_statement(inner, used, next_name, declarations, changed)
                })
                .collect(),
        },
        _ => statement.clone(),
    }
}

struct Plan {
    assignment: usize,
    stride: u32,
    step: i64,
}

impl Plan {
    fn recognize(statement: &Statement) -> Option<Self> {
        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        } = statement
        else {
            return None;
        };
        let index = zero_initializer(initializer)?;
        let (step_index, step_value) = counted_step(step)?;
        if step_index != index
            || !crate::analysis::expression_reads_name(condition, index)
        {
            return None;
        }

        let mut assignment = None;
        let mut stride = None;
        for (position, statement) in body.iter().enumerate() {
            let Statement::Assign {
                value: Expression::Index { base, index: used },
                ..
            } = statement
            else {
                continue;
            };
            let Expression::Member {
                member_type: Type::Struct { size, .. },
                index_stride: None,
                ..
            } = base.as_ref()
            else {
                continue;
            };
            if *size == 0
                || !matches!(used.as_ref(), Expression::Variable(name) if name == index)
            {
                continue;
            }
            if assignment.replace(position).is_some() {
                return None;
            }
            stride = Some(*size);
        }
        let assignment = assignment?;
        let stride = stride?;

        // Retain a second induction variable only when the source index also
        // has an independent use in the loop. Otherwise a pointer cursor or a
        // direct replacement of the source induction variable is cheaper.
        let reads = body
            .iter()
            .map(|statement| statement_name_read_count(statement, index))
            .sum::<usize>();
        if reads < 2 {
            return None;
        }

        Some(Self {
            assignment,
            stride,
            step: step_value,
        })
    }

    fn rewrite(&self, statement: &Statement, offset: &str) -> Statement {
        let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        else {
            unreachable!("member-array offset plan was recognized from a loop")
        };
        let mut body = body.clone();
        let Statement::Assign {
            name,
            value: Expression::Index { base, .. },
        } = &body[self.assignment]
        else {
            unreachable!("recognized member-array assignment changed shape")
        };
        body[self.assignment] = Statement::Assign {
            name: name.clone(),
            value: Expression::Index {
                base: base.clone(),
                index: Box::new(Expression::Variable(offset.to_owned())),
            },
        };
        let offset_step = self
            .step
            .checked_mul(i64::from(self.stride))
            .expect("recognized loop stride fits in i64");
        Statement::Loop {
            kind: *kind,
            initializer: Some(Expression::Comma {
                left: Box::new(initializer.clone().expect("recognized initializer")),
                right: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable(offset.to_owned())),
                    value: Box::new(Expression::IntegerLiteral(0)),
                }),
            }),
            condition: condition.clone(),
            step: Some(Expression::Comma {
                left: Box::new(step.clone().expect("recognized step")),
                right: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable(offset.to_owned())),
                    value: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable(offset.to_owned())),
                        right: Box::new(Expression::IntegerLiteral(offset_step)),
                    }),
                }),
            }),
            body,
        }
    }
}

/// Saved-home ordering for loops that retain both their logical index and a
/// prescaled embedded-array byte offset. The receiver spans the complete
/// function, followed by the two loop-carried values in source-definition
/// order; MWCC assigns those lifetimes to r31, r29, and r30 respectively.
pub(super) struct HomeLayout {
    preferences: std::collections::HashMap<usize, u8>,
    loop_invariant_homes: Option<[u8; 3]>,
}

impl HomeLayout {
    pub(super) fn plan(
        eager_local_count: usize,
        saved_parameter_count: usize,
        deferred_locals: &[&LocalDeclaration],
        deferred_homes: &super::structured_locals::DeferredSavedHomePlan,
        parameter_reuse: &super::structured_parameter_home_reuse::StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        if eager_local_count != 0 || saved_parameter_count != 1 {
            return None;
        }
        if deferred_locals.len() == 3
            && deferred_homes.group_count == 3
            && home_count == 4
        {
            let element = deferred_locals.iter().find(|local| {
                local
                    .name
                    .starts_with(super::structured_loop_member_element_base::PREFIX)
            })?;
            let offset = deferred_locals.iter().find(|local| {
                local
                    .name
                    .starts_with(crate::analysis::PRESCALED_MEMBER_ARRAY_INDEX_PREFIX)
            })?;
            let index = deferred_locals
                .iter()
                .find(|local| local.name != offset.name && local.name != element.name)?;
            let index_home = parameter_reuse.home_index(deferred_homes.group(&index.name));
            let offset_home = parameter_reuse.home_index(deferred_homes.group(&offset.name));
            let element_home = parameter_reuse.home_index(deferred_homes.group(&element.name));
            return Some(Self {
                preferences: std::collections::HashMap::from([
                    (0, 30),
                    (index_home, 31),
                    (offset_home, 29),
                    (element_home, 25),
                ]),
                loop_invariant_homes: Some([26, 27, 28]),
            });
        }
        if deferred_locals.len() != 2
            || deferred_homes.group_count != 2
            || home_count != 3
        {
            return None;
        }
        let cursor = deferred_locals.iter().find(|local| {
            local
                .name
                .starts_with(crate::analysis::PRESCALED_MEMBER_ARRAY_INDEX_PREFIX)
        })?;
        let index = deferred_locals
            .iter()
            .find(|local| local.name != cursor.name)?;
        let index_home = parameter_reuse.home_index(deferred_homes.group(&index.name));
        let cursor_home = parameter_reuse.home_index(deferred_homes.group(&cursor.name));
        Some(Self {
            preferences: std::collections::HashMap::from([
                (0, 31),
                (index_home, 29),
                (cursor_home, 30),
            ]),
            loop_invariant_homes: None,
        })
    }

    pub(super) fn preference(&self, home: usize) -> Option<u8> {
        self.preferences.get(&home).copied()
    }

    pub(super) fn loop_invariant_homes(&self) -> Option<[u8; 3]> {
        self.loop_invariant_homes
    }

    /// Lowest physical register covered by this layout's dense save image.
    /// Sparse semantic homes still make `stmw`/`lmw` preserve every register
    /// between the lowest preference and r31.
    pub(super) fn first_saved_register(&self) -> u8 {
        self.preferences
            .values()
            .copied()
            .min()
            .expect("a member-array home layout has preferences")
    }
}

impl Generator {
    /// Reproduce the local instruction interleaving MWCC uses around the
    /// retained logical index/byte-offset loop. Recognition is deliberately
    /// gated by [`HomeLayout`]: these moves describe the scheduling region
    /// exposed by that allocation plan, not general peephole preferences.
    pub(crate) fn schedule_member_array_offset_loop(&mut self) {
        self.spell_member_array_receiver_copies_as_moves();
        self.schedule_member_array_callback_publication();
        self.schedule_member_array_outer_call_arguments();
        self.schedule_member_array_loop_entry();
        self.schedule_member_array_short_loop_entry();
        self.schedule_member_array_loop_call_arguments();
        self.schedule_member_array_tail_call();
        self.schedule_member_array_tail_stores();
    }

    fn spell_member_array_receiver_copies_as_moves(&mut self) {
        for instruction in &mut self.output.instructions {
            let Instruction::AddImmediate {
                d,
                a,
                immediate: 0,
            } = *instruction
            else {
                continue;
            };
            if (d == 31 && a == Eabi::FIRST_GENERAL_ARGUMENT)
                || (d == Eabi::FIRST_GENERAL_ARGUMENT && a == 31)
            {
                *instruction = Instruction::move_register(d, a);
            }
        }
        if let Some(start) = self.output.instructions.windows(2).position(|window| {
            matches!(
                window,
                [
                    Instruction::Or { a: 31, s: 3, b: 3 },
                    Instruction::LoadWord { d: 0, a: 31, offset: 32 },
                ]
            )
        }) {
            let Instruction::LoadWord { a, .. } = &mut self.output.instructions[start + 1]
            else {
                unreachable!("receiver load changed after recognition")
            };
            *a = Eabi::FIRST_GENERAL_ARGUMENT;
        }
    }

    fn schedule_member_array_callback_publication(&mut self) {
        let Some(start) = self.output.instructions.windows(7).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                    Instruction::AddImmediate { d: 0, a: 4, .. },
                    Instruction::StoreWord { s: 0, a: 3, .. },
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
                    Instruction::StoreHalfword { s: 0, a: 3, .. },
                ]
            )
        }) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, start + 1, start);
        // The callback address high is part of the fallthrough block headed by
        // the receiver load. An incoming success edge must enter the newly
        // widened block at the moved high half, not preserve the old load as
        // its semantic label owner.
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == start + 1 =>
                {
                    *target = start;
                }
                _ => {}
            }
        }
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start + 2]
        else {
            unreachable!("callback low half changed after recognition")
        };
        *d = 4;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 3]
        else {
            unreachable!("callback store changed after recognition")
        };
        *s = 4;
        crate::move_instruction_before_retargeting(self, start + 5, start + 3);
    }

    fn schedule_member_array_outer_call_arguments(&mut self) {
        if let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadByteZero { d: 3, a: 3, offset: 0 },
                    Instruction::LoadWord { d: 4, a: 31, .. },
                    Instruction::LoadWord { d: 5, a: 31, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) {
            crate::move_instruction_before_retargeting(self, start + 2, start + 1);
        }
        if let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadByteZero { d: 3, a: 3, offset: 0 },
                    Instruction::LoadWord { d: 4, a: 31, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) {
            crate::move_instruction_before_retargeting(self, start + 2, start + 1);
        }
    }

    fn schedule_member_array_loop_entry(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 30, .. },
                    Instruction::LoadHalfwordZeroIndexed { d: 3, a: 31, b: 3 },
                    Instruction::StoreHalfword { s: 3, a: 1, .. },
                    Instruction::LoadWord { d: 0, .. },
                    Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                ]
            )
        }) else {
            return;
        };
        let global_load = start + 3;
        if !self.output.relocations.iter().any(|relocation| {
            relocation.instruction_index == global_load
                && relocation.kind == RelocationKind::EmbSda21
        }) {
            return;
        }

        crate::move_instruction_before_retargeting(self, global_load, start + 1);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
    }

    fn schedule_member_array_loop_call_arguments(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadByteZero { d: 3, a: 3, offset: 0 },
                    Instruction::ClearLeftImmediate { a: 4, s: 29, clear: 24 },
                    Instruction::LoadByteZero { d: 5, a: 1, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, start + 2, start + 1);
        crate::move_instruction_before_retargeting(self, start + 3, start + 2);
    }

    fn schedule_member_array_short_loop_entry(&mut self) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 30, .. },
                    Instruction::LoadHalfwordZeroIndexed { d: 3, a: 31, b: 3 },
                    Instruction::StoreHalfword { s: 3, a: 1, .. },
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadByteZero { d: 3, a: 3, offset: 0 },
                    Instruction::ClearLeftImmediate { a: 4, s: 29, clear: 24 },
                ]
            )
        }) else {
            return;
        };

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start] else {
            unreachable!("short-loop member offset changed after recognition")
        };
        *d = GENERAL_SCRATCH;
        let Instruction::LoadHalfwordZeroIndexed { d, b, .. } =
            &mut self.output.instructions[start + 1]
        else {
            unreachable!("short-loop aggregate load changed after recognition")
        };
        *d = GENERAL_SCRATCH;
        *b = GENERAL_SCRATCH;
        let Instruction::StoreHalfword { s, .. } = &mut self.output.instructions[start + 2]
        else {
            unreachable!("short-loop aggregate store changed after recognition")
        };
        *s = GENERAL_SCRATCH;
        crate::move_instruction_before_retargeting(self, start + 5, start + 1);
        crate::move_instruction_before_retargeting(self, start + 6, start + 5);
    }

    fn schedule_member_array_tail_call(&mut self) {
        let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 0, a: 31, .. },
                    Instruction::StoreWord { s: 0, a: 31, .. },
                    Instruction::Or { a: 3, s: 31, b: 31 },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 2, start + 1);
    }

    fn schedule_member_array_tail_stores(&mut self) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadWord { d: 0, a: 31, .. },
                    Instruction::StoreByte { s: 0, a: 3, .. },
                    Instruction::LoadWord { d: 3, a: 31, offset: 32 },
                    Instruction::LoadHalfwordZero { d: 0, a: 31, .. },
                    Instruction::StoreHalfword { s: 0, a: 3, .. },
                ]
            )
        }) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, start + 1, start);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
    }
}

fn zero_initializer(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    (crate::analysis::constant_value(value) == Some(0)).then_some(index)
}

fn counted_step(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
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
    (amount > 0
        && matches!(left.as_ref(), Expression::Variable(name) if name == index))
    .then_some((index, amount))
}

fn statement_name_read_count(statement: &Statement, name: &str) -> usize {
    let mut count = 0;
    super::structured_expression_visit::visit_statement(statement, &mut |expression| {
        if matches!(expression, Expression::Variable(read) if read == name) {
            count += 1;
        }
    });
    count
}

fn fresh_name(
    used: &mut std::collections::HashSet<String>,
    next: &mut usize,
) -> String {
    loop {
        let name = format!(
            "{}{}",
            crate::analysis::PRESCALED_MEMBER_ARRAY_INDEX_PREFIX,
            *next,
        );
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted_member_array_loop(independent_index_read: bool) -> Statement {
        let mut body = vec![Statement::Assign {
            name: "element".into(),
            value: Expression::Index {
                base: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 264,
                    member_type: Type::Struct { size: 2, align: 2 },
                    index_stride: None,
                }),
                index: Box::new(Expression::Variable("index".into())),
            },
        }];
        if independent_index_read {
            body.push(Statement::Expression(Expression::Variable("index".into())));
        }
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("index".into())),
                right: Box::new(Expression::IntegerLiteral(6)),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("index".into())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("index".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body,
        }
    }

    #[test]
    fn recognizes_independent_logical_and_byte_offset_inductions() {
        let statement = counted_member_array_loop(true);
        let plan = Plan::recognize(&statement).expect("member-array loop should be reduced");
        assert_eq!(plan.stride, 2);
        assert_eq!(plan.step, 1);

        let rewritten = plan.rewrite(&statement, "@member_offset");
        let Statement::Loop {
            initializer: Some(Expression::Comma { .. }),
            step: Some(Expression::Comma { right, .. }),
            body,
            ..
        } = rewritten
        else {
            panic!("reduced loop should carry both induction variables")
        };
        assert!(matches!(
            right.as_ref(),
            Expression::Assign { value, .. }
                if matches!(
                    value.as_ref(),
                    Expression::Binary { right, .. }
                        if matches!(right.as_ref(), Expression::IntegerLiteral(2))
                )
        ));
        assert!(matches!(
            &body[0],
            Statement::Assign {
                value: Expression::Index { index, .. },
                ..
            } if matches!(index.as_ref(), Expression::Variable(name) if name == "@member_offset")
        ));
    }

    #[test]
    fn leaves_single_use_indices_to_the_ordinary_index_lowerer() {
        assert!(Plan::recognize(&counted_member_array_loop(false)).is_none());
    }
}
