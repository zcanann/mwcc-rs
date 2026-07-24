//! Loop-site expansion of a guarded call with entry-parameter survivors.
//!
//! A small helper expanded inside a list walk leaves four independent values
//! live across its condition and selected-arm calls: the current object, the
//! iterator, and two scalar entry arguments. This owner supplies that CFG
//! liveness; the ordinary condition and statement emitters still own the
//! helper's actual predicates and call arguments.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_inlined_short_circuit_call_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || function.parameters.len() != 2
            || function
                .parameters
                .iter()
                .any(|parameter| class_of(parameter.parameter_type).ok() != Some(ValueClass::General))
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(loop_condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some(iterator_name) = nonnull_loop_variable(loop_condition) else {
            return Ok(false);
        };
        let Some(iterator_local) = function
            .locals
            .iter()
            .find(|local| local.name == iterator_name)
        else {
            return Ok(false);
        };
        let Some((head_global, head_offset)) = global_member_initializer(iterator_local) else {
            return Ok(false);
        };
        let [
            Statement::Assign {
                name: object_name,
                value:
                    Expression::Member {
                        base: object_base,
                        offset: object_offset,
                        member_type: object_type,
                        index_stride: None,
                    },
            },
            Statement::Assign {
                name: alias_name,
                value: Expression::Variable(alias_source),
            },
            Statement::If {
                condition,
                then_body,
                else_body,
            },
            Statement::Assign {
                name: next_name,
                value:
                    Expression::Member {
                        base: next_base,
                        offset: next_offset,
                        member_type: next_type,
                        index_stride: None,
                    },
            },
        ] = body.as_slice()
        else {
            return Ok(false);
        };
        let (Ok(object_offset), Ok(next_offset)) =
            (i16::try_from(*object_offset), i16::try_from(*next_offset))
        else {
            return Ok(false);
        };
        if !else_body.is_empty()
            || then_body.len() != 1
            || !expression_has_call(condition)
            || !matches!(condition, Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                ..
            })
            || alias_source != object_name
            || next_name != &iterator_name
            || !matches!(object_base.as_ref(), Expression::Variable(name) if name == &iterator_name)
            || !matches!(next_base.as_ref(), Expression::Variable(name) if name == &iterator_name)
            || !matches!(object_type, Type::Pointer(_) | Type::StructPointer { .. })
            || !matches!(next_type, Type::Pointer(_) | Type::StructPointer { .. })
            || !matches!(then_body.as_slice(), [Statement::Expression(Expression::Call { .. })])
        {
            return Ok(false);
        }

        let Some(incoming_first) = self.lookup_general(&function.parameters[0].name) else {
            return Ok(false);
        };
        let Some(incoming_second) = self.lookup_general(&function.parameters[1].name) else {
            return Ok(false);
        };
        let iterator = self.fresh_virtual_general();
        let object = self.fresh_virtual_general();
        let second = self.fresh_virtual_general();
        let first = self.fresh_virtual_general();
        // MWCC colors the loop-carried iterator first, followed by the selected
        // object and the entry arguments. All synthesized inline locals are
        // register-promoted, so they do not reserve a stack-local region.
        let homes = vec![iterator, object, second, first];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        // This owner has incorporated the expanded helper's retained local
        // region into `plan`; the generic post-pass must not add it again.
        self.legacy_inline_expansion_frame_bytes = 0;
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::PreserveLogicalSize;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
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
        for (slot, home) in homes.iter().copied().enumerate() {
            self.output.instructions.push(Instruction::StoreWord {
                s: home,
                a: 1,
                offset: plan.frame_size - 4 * (slot as i16 + 1),
            });
            let incoming = match slot {
                2 => Some(incoming_second),
                3 => Some(incoming_first),
                _ => None,
            };
            if let Some(incoming) = incoming {
                self.output
                    .instructions
                    .push(Instruction::move_register(home, incoming));
            }
        }

        for (name, register, declared_type) in [
            (object_name, object, *object_type),
            (alias_name, object, *object_type),
            (&iterator_name, iterator, iterator_local.declared_type),
            (&function.parameters[0].name, first, function.parameters[0].parameter_type),
            (&function.parameters[1].name, second, function.parameters[1].parameter_type),
        ] {
            self.locations.insert(
                name.clone(),
                Location {
                    class: ValueClass::General,
                    register,
                    signed: self.signed_of(declared_type),
                    width: declared_type.width(),
                    pointee: match declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(declared_type),
                },
            );
        }

        self.record_relocation(RelocationKind::EmbSda21, &head_global);
        self.output.instructions.push(Instruction::LoadWord {
            d: iterator,
            a: 0,
            offset: head_offset,
        });

        let test = self.fresh_label();
        let loop_body = self.fresh_label();
        self.emit_branch_to(test);
        self.bind_label(loop_body);
        self.output.instructions.push(Instruction::LoadWord {
            d: object,
            a: iterator,
            offset: object_offset,
        });
        let skip_call = self.fresh_label();
        let mut condition_terms = Vec::new();
        collect_logical_and_terms(condition, &mut condition_terms);
        for term in condition_terms {
            let term_start = self.output.instructions.len();
            let (options, condition_bit) = self.emit_condition_test(term)?;
            schedule_three_argument_loop_call(&mut self.output.instructions[term_start..]);
            self.emit_branch_conditional_to(options, condition_bit, skip_call);
        }
        self.emit_statement(&then_body[0])?;
        self.bind_label(skip_call);
        self.output.instructions.push(Instruction::LoadWord {
            d: iterator,
            a: iterator,
            offset: next_offset,
        });
        self.bind_label(test);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: iterator,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

/// MWCC forwards the two independent entry arguments before placing the loop's
/// selected object into r3. The generic call emitter starts with r3; normalize
/// this three-survivor group while all source identities are still explicit.
fn schedule_three_argument_loop_call(instructions: &mut [Instruction]) -> bool {
    if instructions.len() < 4 {
        return false;
    }
    for index in 0..=instructions.len() - 4 {
        let (object, first, second) = match &instructions[index..index + 4] {
            [
                Instruction::Or { a: 3, s: object, b: object_b },
                Instruction::Or { a: 4, s: first, b: first_b },
                Instruction::Or { a: 5, s: second, b: second_b },
                Instruction::BranchAndLink { .. },
            ] if object == object_b && first == first_b && second == second_b => {
                (*object, *first, *second)
            }
            _ => continue,
        };
        instructions[index] = Instruction::AddImmediate {
            d: 4,
            a: first,
            immediate: 0,
        };
        instructions[index + 1] = Instruction::AddImmediate {
            d: 5,
            a: second,
            immediate: 0,
        };
        instructions[index + 2] = Instruction::AddImmediate {
            d: 3,
            a: object,
            immediate: 0,
        };
        return true;
    }
    false
}

fn nonnull_loop_variable(condition: &Expression) -> Option<String> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), Expression::IntegerLiteral(0))
        | (Expression::IntegerLiteral(0), Expression::Variable(name)) => Some(name.clone()),
        _ => None,
    }
}

fn global_member_initializer(local: &LocalDeclaration) -> Option<(String, i16)> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = local.initializer.as_ref()?
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global.clone(), i16::try_from(*offset).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_independent_loop_call_arguments_before_the_receiver() {
        let mut instructions = vec![
            Instruction::move_register(3, 40),
            Instruction::move_register(4, 41),
            Instruction::move_register(5, 42),
            Instruction::BranchAndLink {
                target: "predicate".into(),
            },
        ];

        assert!(schedule_three_argument_loop_call(&mut instructions));
        assert!(matches!(instructions.as_slice(), [
            Instruction::AddImmediate { d: 4, a: 41, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 42, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 40, immediate: 0 },
            Instruction::BranchAndLink { target },
        ] if target == "predicate"));
    }
}

fn collect_logical_and_terms<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = expression
    {
        collect_logical_and_terms(left, terms);
        collect_logical_and_terms(right, terms);
    } else {
        terms.push(expression);
    }
}
