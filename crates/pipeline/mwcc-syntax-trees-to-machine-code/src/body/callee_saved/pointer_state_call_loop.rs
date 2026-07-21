//! Pointer-list walks with a conditional call on selected node states.
//!
//! The current node remains in the first argument/result register. Its successor
//! is loaded before the call and is therefore the one loop-carried value that
//! needs a callee-saved virtual home.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

impl Generator {
    /// Lower the SDK cancellation walk:
    ///
    /// ```c
    /// for (node = head; node; node = next) {
    ///     next = node->next;
    ///     switch (node->state) { case A: case B: cancel(node); }
    /// }
    /// ```
    ///
    /// CodeWarrior keeps `node` in r3 and colors only `next` callee-saved. The
    /// two adjacent cases use its small comparison tree instead of a jump table.
    pub(crate) fn try_pointer_state_call_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let [node_local, next_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if node_local.initializer.is_some()
            || next_local.initializer.is_some()
            || node_local.array_length.is_some()
            || next_local.array_length.is_some()
            || node_local.is_static
            || next_local.is_static
            || !matches!(node_local.declared_type, Type::StructPointer { .. })
            || !matches!(next_local.declared_type, Type::StructPointer { .. })
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::For,
            initializer:
                Some(Expression::Assign {
                    target,
                    value: head,
                }),
            condition: Some(Expression::Variable(condition)),
            step:
                Some(Expression::Assign {
                    target: step_target,
                    value: step_value,
                }),
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(target.as_ref(), Expression::Variable(name) if name == &node_local.name)
            || condition != &node_local.name
            || !matches!(step_target.as_ref(), Expression::Variable(name) if name == &node_local.name)
            || !matches!(step_value.as_ref(), Expression::Variable(name) if name == &next_local.name)
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: assigned_next,
            value:
                Expression::Member {
                    base: next_base,
                    offset: next_offset,
                    member_type: next_type,
                    ..
                },
        }, Statement::Switch {
            scrutinee:
                Expression::Member {
                    base: state_base,
                    offset: state_offset,
                    member_type: state_type,
                    ..
                },
            arms,
            default: None,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        if assigned_next != &next_local.name
            || !matches!(next_base.as_ref(), Expression::Variable(name) if name == &node_local.name)
            || !matches!(state_base.as_ref(), Expression::Variable(name) if name == &node_local.name)
            || !matches!(next_type, Type::Pointer(_) | Type::StructPointer { .. })
            || !matches!(
                state_type,
                Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Int
                    | Type::UnsignedInt
            )
        {
            return Ok(false);
        }
        let [first, second] = arms.as_slice() else {
            return Ok(false);
        };
        let (low, high) = if first.value < second.value {
            (first, second)
        } else {
            (second, first)
        };
        if low.value >= high.value
            || !low.falls_through
            || !matches!(&low.body, ArmBody::Statements(statements) if statements.is_empty())
            || high.falls_through
        {
            return Ok(false);
        }
        let ArmBody::Statements(high_body) = &high.body else {
            return Ok(false);
        };
        let [Statement::Expression(Expression::Call {
            name: callee,
            arguments,
        })] = high_body.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(arguments.as_slice(), [Expression::Variable(name)] if name == &node_local.name)
            || i16::try_from(low.value).is_err()
            || i16::try_from(high.value).is_err()
        {
            return Ok(false);
        }

        let saved_next = self.fresh_virtual_general();
        let plan = mwcc_vreg::FramePlan::sized_for(vec![saved_next]);
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = plan.saved.clone();
        self.output.instructions.extend(plan.prologue());
        self.output.pre_scheduled = true;
        // The for scaffold and two-case comparison tree consume seven legacy
        // anonymous control-flow ordinals even when their labels collapse.
        self.output.anonymous_label_bump = 7;

        self.locations.insert(
            node_local.name.clone(),
            Location {
                class: ValueClass::General,
                register: Eabi::general_result().number,
                signed: false,
                width: 32,
                pointee: None,
                stride: pointer_stride(node_local.declared_type),
            },
        );
        self.locations.insert(
            next_local.name.clone(),
            Location {
                class: ValueClass::General,
                register: saved_next,
                signed: false,
                width: 32,
                pointee: None,
                stride: pointer_stride(next_local.declared_type),
            },
        );

        let node = Eabi::general_result().number;
        self.evaluate(head, node_local.declared_type, node)?;

        // The for-loop's retained source edges. The first two are adjacent
        // one-instruction blocks; the third jumps to the bottom test.
        let edge_one = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch {
            target: edge_one + 1,
        });
        let edge_two = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch {
            target: edge_two + 1,
        });
        let to_test = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let body_start = self.output.instructions.len();
        self.evaluate(
            &Expression::Member {
                base: Box::new(Expression::Variable(node_local.name.clone())),
                offset: *state_offset,
                member_type: *state_type,
                index_stride: None,
            },
            *state_type,
            0,
        )?;
        self.evaluate(
            &Expression::Member {
                base: Box::new(Expression::Variable(node_local.name.clone())),
                offset: *next_offset,
                member_type: *next_type,
                index_stride: None,
            },
            *next_type,
            saved_next,
        )?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: high.value as i16,
            });
        let equal_high = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        let above_low = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 0,
                immediate: low.value as i16,
            });
        let equal_low = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        let skip_call = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let call_at = self.output.instructions.len();
        self.record_relocation(RelocationKind::Rel24, &callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        let after_call = self.output.instructions.len();
        for index in [equal_high, equal_low] {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[index]
            {
                *target = call_at;
            }
        }
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[above_low]
        {
            *target = after_call;
        }
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_call] {
            *target = after_call;
        }

        self.output
            .instructions
            .push(Instruction::move_register(node, saved_next));
        let test_at = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[to_test] {
            *target = test_at;
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: node,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: body_start,
            });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
