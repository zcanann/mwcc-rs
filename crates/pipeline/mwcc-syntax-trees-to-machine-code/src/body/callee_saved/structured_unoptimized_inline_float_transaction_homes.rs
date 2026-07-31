//! O0 saved-FPR homes retained by composed scalar/vector transactions.
//!
//! Automatic value inlining turns the source bindings into hygienic locals.
//! MWCC still allocates the projection result, divisor, caller result, and the
//! following interpolation amount as distinct source-image values. This plan
//! recovers that semantic chain before instruction selection without applying
//! decompiler-name policy to unrelated inline expansions.

use super::*;

struct PhysicalHandoffShape {
    projection: usize,
    interpolation: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Vec3CopyPacket {
    source: u8,
    target_offset: i16,
}

pub(super) struct StructuredUnoptimizedInlineFloatTransactionHomes {
    caller_result: String,
    projection_result: String,
    projection_divisor: String,
    interpolation_amount: String,
}

impl Generator {
    /// Restore the distinct assignment-value lane that MWCC retains at O0 and
    /// issue the complete scalar handoff before copying aggregate arguments.
    /// The same source transaction needs only its occupied 80-byte local area;
    /// generic inline residue otherwise leaves one unused 16-byte band.
    pub(crate) fn schedule_unoptimized_inline_float_transaction_handoffs(&mut self) {
        if !self.unoptimized_inline_float_transaction_homes
            || self.frame_size != 96
            || !self.callee_saved.is_empty()
        {
            return;
        }
        let Some(shape) = physical_handoff_shape(&self.output.instructions) else {
            return;
        };
        if self.frame_slots.values().any(|slot| {
            i32::from(slot.offset) + i32::try_from(slot.size).unwrap_or(i32::MAX) > 80
        }) {
            return;
        }
        if !self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -96
                }
            )
        }) || !self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 96
                }
            )
        }) {
            return;
        }
        crate::remove_instruction_retargeting_to_next(self, shape.interpolation);
        crate::insert_instruction_retargeting(
            self,
            shape.projection + 1,
            Instruction::FloatMove { d: 28, b: 27 },
        );
        crate::insert_instruction_retargeting(
            self,
            shape.projection + 2,
            Instruction::FloatMove { d: 31, b: 28 },
        );
        if let Some(copy_start) = (shape.projection + 3..self.output.instructions.len())
            .find(|start| {
                vec3_copy_packet(&self.output.instructions, *start)
                    == Some(Vec3CopyPacket {
                        source: 4,
                        target_offset: 20,
                    })
                    && vec3_copy_packet(&self.output.instructions, *start + 6)
                        == Some(Vec3CopyPacket {
                            source: 5,
                            target_offset: 8,
                        })
            })
        {
            for index in 0..6 {
                crate::move_instruction_before_retargeting(
                    self,
                    copy_start + 6 + index,
                    copy_start + index,
                );
            }
        }
        let frame_push = self
            .output
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -96
                    }
                )
            })
            .expect("validated transaction frame push disappeared");
        let frame_pop = self
            .output
            .instructions
            .iter()
            .rposition(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 96
                    }
                )
            })
            .expect("validated transaction frame pop disappeared");
        self.output.instructions[frame_push] = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -80,
        };
        self.output.instructions[frame_pop] = Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 80,
        };
        self.frame_size = 80;
    }
}

fn vec3_copy_packet(instructions: &[Instruction], start: usize) -> Option<Vec3CopyPacket> {
    let [
        Instruction::LoadWord { d: 6, a: source0, offset: source_offset0 },
        Instruction::LoadWord { d: 0, a: source1, offset: source_offset1 },
        Instruction::StoreWord { s: 6, a: 1, offset: target_offset0 },
        Instruction::StoreWord { s: 0, a: 1, offset: target_offset1 },
        Instruction::LoadWord { d: 0, a: source2, offset: source_offset2 },
        Instruction::StoreWord { s: 0, a: 1, offset: target_offset2 },
    ] = instructions.get(start..start.checked_add(6)?)?
    else {
        return None;
    };
    (*source0 == *source1
        && *source0 == *source2
        && source_offset0.checked_add(4) == Some(*source_offset1)
        && source_offset0.checked_add(8) == Some(*source_offset2)
        && target_offset0.checked_add(4) == Some(*target_offset1)
        && target_offset0.checked_add(8) == Some(*target_offset2))
    .then_some(Vec3CopyPacket {
        source: *source0,
        target_offset: *target_offset0,
    })
}

fn physical_handoff_shape(instructions: &[Instruction]) -> Option<PhysicalHandoffShape> {
    let projection = instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::FloatMove { d: 27, b: 30 })
    })?;
    let interpolation = instructions
        .iter()
        .enumerate()
        .skip(projection + 1)
        .find_map(|(index, instruction)| {
            matches!(instruction, Instruction::FloatMove { d: 31, b: 27 })
                .then_some(index)
        })?;
    (!instructions[projection + 1..interpolation]
        .iter()
        .any(|instruction| {
            matches!(
                instruction,
                Instruction::FloatMove { .. }
                    | Instruction::BranchAndLink { .. }
                    | Instruction::BranchToLinkRegisterAndLink
                    | Instruction::BranchToCountRegisterAndLink
            )
        }))
    .then_some(PhysicalHandoffShape {
        projection,
        interpolation,
    })
}

impl StructuredUnoptimizedInlineFloatTransactionHomes {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        if function_makes_call(function) || !function.guards.is_empty() {
            return None;
        }
        let [leading, Statement::Assign { name: caller_result, value: projection }, interpolation, Statement::Return(Some(_))] =
            function.statements.as_slice()
        else {
            return None;
        };
        if !matches!(leading, Statement::If { .. })
            || !matches!(interpolation, Statement::Expression(_))
        {
            return None;
        }
        let caller = function.locals.iter().find(|local| {
            local.name == *caller_result
                && local.declared_type == Type::Float
                && !local.name.starts_with("__mwcc_inline_")
        })?;
        if caller.initializer.is_some() || caller.is_static || caller.array_length.is_some() {
            return None;
        }
        let inline_vectors = function.locals.iter().filter(|local| {
            local.name.starts_with("__mwcc_inline_")
                && matches!(local.declared_type, Type::Struct { size: 12, .. })
        });
        if inline_vectors.count() != 5
            || function
                .locals
                .iter()
                .filter(|local| {
                    !local.name.starts_with("__mwcc_inline_")
                        && matches!(local.declared_type, Type::Struct { size: 12, .. })
                })
                .count()
                != 1
        {
            return None;
        }
        let inline_floats = function
            .locals
            .iter()
            .filter(|local| {
                local.name.starts_with("__mwcc_inline_")
                    && local.declared_type == Type::Float
                    && local.initializer.is_none()
                    && !local.is_static
                    && local.array_length.is_none()
            })
            .collect::<Vec<_>>();
        if inline_floats.len() != 3 {
            return None;
        }
        let Statement::Expression(interpolation) = interpolation else {
            unreachable!("interpolation statement shape was checked")
        };
        let projection_result_name = terminal_variable(projection)?;
        let projection_result = inline_floats
            .iter()
            .find(|local| local.name == projection_result_name)?;
        let interpolation_amount = inline_floats.iter().find(|local| {
            assigns_variable_from(interpolation, &local.name, &caller.name)
        })?;
        let projection_divisor = inline_floats.iter().find(|local| {
            local.name != projection_result.name && local.name != interpolation_amount.name
        })?;
        if !expression_assigns_name(projection, &projection_result.name)
            || !expression_assigns_name(projection, &projection_divisor.name)
        {
            return None;
        }
        Some(Self {
            caller_result: caller.name.clone(),
            projection_result: projection_result.name.clone(),
            projection_divisor: projection_divisor.name.clone(),
            interpolation_amount: interpolation_amount.name.clone(),
        })
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        if name == self.caller_result {
            Some(27)
        } else if name == self.projection_result {
            Some(30)
        } else if name == self.projection_divisor {
            Some(29)
        } else if name == self.interpolation_amount {
            Some(31)
        } else {
            None
        }
    }

    pub(super) fn saved_count(&self) -> u8 {
        5
    }
}

fn terminal_variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Comma { right, .. } => terminal_variable(right),
        _ => None,
    }
}

fn expression_assigns_name(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Assign { target, value } => {
            matches!(target.as_ref(), Expression::Variable(name) if name == expected)
                || expression_assigns_name(value, expected)
        }
        Expression::Comma { left, right } => {
            expression_assigns_name(left, expected) || expression_assigns_name(right, expected)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_assigns_name(condition, expected)
                || expression_assigns_name(when_true, expected)
                || expression_assigns_name(when_false, expected)
        }
        Expression::Binary { left, right, .. } => {
            expression_assigns_name(left, expected) || expression_assigns_name(right, expected)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            expression_assigns_name(operand, expected)
        }
        _ => false,
    }
}

fn assigns_variable_from(expression: &Expression, target: &str, source: &str) -> bool {
    match expression {
        Expression::Assign { target: assigned, value } => {
            matches!(assigned.as_ref(), Expression::Variable(name) if name == target)
                && matches!(value.as_ref(), Expression::Variable(name) if name == source)
        }
        Expression::Comma { left, right } => {
            assigns_variable_from(left, target, source)
                || assigns_variable_from(right, target, source)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn plans_only_the_complete_projection_interpolation_transaction() {
        let vector = Type::Struct { size: 12, align: 4 };
        let projection_result = "__mwcc_inline_project_4_var_f31";
        let projection_divisor = "__mwcc_inline_project_3_temp_f30";
        let amount = "__mwcc_inline_interpolate_7_arg8";
        let mut locals = vec![local("output", vector), local("result", Type::Float)];
        locals.extend((0..5).map(|index| {
            local(&format!("__mwcc_inline_vector_{index}"), vector)
        }));
        locals.extend([
            local(projection_divisor, Type::Float),
            local(projection_result, Type::Float),
            local(amount, Type::Float),
        ]);
        let assign = |target: &str, value| Expression::Assign {
            target: Box::new(Expression::Variable(target.into())),
            value: Box::new(value),
        };
        let function = Function {
            return_type: Type::Float,
            name: "distance".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals,
            statements: vec![
                Statement::If {
                    condition: Expression::IntegerLiteral(1),
                    then_body: Vec::new(),
                    else_body: Vec::new(),
                },
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::Comma {
                        left: Box::new(assign(
                            projection_divisor,
                            Expression::FloatLiteral(-1.0),
                        )),
                        right: Box::new(Expression::Comma {
                            left: Box::new(assign(
                                projection_result,
                                Expression::FloatLiteral(1.0),
                            )),
                            right: Box::new(Expression::Variable(projection_result.into())),
                        }),
                    },
                },
                Statement::Expression(Expression::Comma {
                    left: Box::new(assign(
                        amount,
                        Expression::Variable("result".into()),
                    )),
                    right: Box::new(Expression::Variable(amount.into())),
                }),
                Statement::Return(Some(Expression::FloatLiteral(0.0))),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let plan = StructuredUnoptimizedInlineFloatTransactionHomes::plan(&function)
            .expect("semantic transaction");
        assert_eq!(plan.preference(projection_result), Some(30));
        assert_eq!(plan.preference(projection_divisor), Some(29));
        assert_eq!(plan.preference("result"), Some(27));
        assert_eq!(plan.preference(amount), Some(31));
        assert_eq!(plan.saved_count(), 5);
    }

    #[test]
    fn recognizes_a_projection_followed_by_a_delayed_interpolation_handoff() {
        let instructions = vec![
            Instruction::FloatMove { d: 27, b: 30 },
            Instruction::LoadWord { d: 6, a: 4, offset: 0 },
            Instruction::StoreWord { s: 6, a: 1, offset: 20 },
            Instruction::FloatMove { d: 31, b: 27 },
        ];

        let shape = physical_handoff_shape(&instructions).expect("handoff shape");
        assert_eq!(shape.projection, 0);
        assert_eq!(shape.interpolation, 3);
    }

    #[test]
    fn recognizes_a_pipelined_vec3_copy_packet() {
        let instructions = vec![
            Instruction::LoadWord { d: 6, a: 5, offset: 0 },
            Instruction::LoadWord { d: 0, a: 5, offset: 4 },
            Instruction::StoreWord { s: 6, a: 1, offset: 8 },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 5, offset: 8 },
            Instruction::StoreWord { s: 0, a: 1, offset: 16 },
        ];

        assert_eq!(
            vec3_copy_packet(&instructions, 0),
            Some(Vec3CopyPacket {
                source: 5,
                target_offset: 8,
            })
        );
    }

    #[test]
    fn rejects_a_handoff_crossing_another_float_transfer() {
        let instructions = vec![
            Instruction::FloatMove { d: 27, b: 30 },
            Instruction::FloatMove { d: 2, b: 1 },
            Instruction::FloatMove { d: 31, b: 27 },
        ];

        assert!(physical_handoff_shape(&instructions).is_none());
    }
}
