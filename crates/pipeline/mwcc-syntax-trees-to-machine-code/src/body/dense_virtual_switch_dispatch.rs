//! Dense virtual-call switch dispatch.
//!
//! Header-only receiver templates commonly decode one integer tag and forward
//! the same payload through a virtual slot per case. The complete switch owns
//! the jump table, indirect-call sequence, and shared linkage epilogue; keeping
//! that transaction together prevents the general statement emitter from
//! treating each source `break` as an unrelated control-flow problem.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{JumpTable, RelocationTarget};

#[derive(Debug, PartialEq, Eq)]
struct DenseVirtualSwitch {
    member_offset: u32,
    arms: Vec<(i64, u16)>,
}

fn recognize(function: &Function) -> Option<DenseVirtualSwitch> {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [this, first, second] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        (this.parameter_type, first.parameter_type, second.parameter_type),
        (
            Type::StructPointer { .. },
            Type::StructPointer { .. } | Type::Pointer(_),
            Type::StructPointer { .. } | Type::Pointer(_)
        )
    ) {
        return None;
    }
    let [Statement::Switch {
        scrutinee:
            Expression::Member {
                base,
                offset: member_offset,
                member_type,
                index_stride: None,
            },
        arms,
        default: None,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == &second.name)
        || !matches!(member_type, Type::Int | Type::UnsignedInt)
        || arms.len() < 7
    {
        return None;
    }

    let mut dispatch = Vec::with_capacity(arms.len());
    for arm in arms {
        let mwcc_syntax_trees::ArmBody::Statements(statements) = &arm.body else {
            return None;
        };
        let [Statement::Expression(Expression::VirtualCall {
            object,
            vptr_offset: 0,
            slot_offset,
            return_type: Type::Void,
            variadic: false,
            arguments,
        })] = statements.as_slice()
        else {
            return None;
        };
        if arm.falls_through
            || !matches!(object.as_ref(), Expression::Variable(name) if name == &this.name)
            || !matches!(arguments.as_slice(),
                [Expression::Variable(left), Expression::Variable(right)]
                    if left == &first.name && right == &second.name)
        {
            return None;
        }
        dispatch.push((arm.value, *slot_offset));
    }
    let mut values = dispatch.iter().map(|(value, _)| *value).collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    if values.len() != dispatch.len()
        || values.first() != Some(&0)
        || values.last().copied()? != values.len() as i64 - 1
    {
        return None;
    }
    Some(DenseVirtualSwitch {
        member_offset: *member_offset,
        arms: dispatch,
    })
}

impl Generator {
    pub(crate) fn try_dense_virtual_switch_dispatch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        let Some(dispatch) = recognize(function) else {
            return Ok(false);
        };
        if dispatch.member_offset > i16::MAX as u32 {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 8;
        self.owns_link_register_schedule = true;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: dispatch.member_offset as i16,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: dispatch.arms.len() as u16 - 1,
            });
        let out_of_range = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 0,
            });
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            });
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: 0,
            a: 6,
            b: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);

        let mut body_offsets = Vec::with_capacity(dispatch.arms.len());
        let mut joins = Vec::with_capacity(dispatch.arms.len().saturating_sub(1));
        for (source_index, (_, slot_offset)) in dispatch.arms.iter().enumerate() {
            body_offsets.push(self.output.instructions.len() as u32 * 4);
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: 3,
                offset: 0,
            });
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: 12,
                offset: *slot_offset as i16,
            });
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
            if source_index + 1 != dispatch.arms.len() {
                joins.push(self.output.instructions.len());
                self.output.instructions.push(Instruction::Branch { target: 0 });
            }
        }

        let epilogue = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[out_of_range]
        {
            *target = epilogue;
        }
        for join in joins {
            let Instruction::Branch { target } = &mut self.output.instructions[join] else {
                unreachable!()
            };
            *target = epilogue;
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        let mut entries = vec![0u32; dispatch.arms.len()];
        for (source_index, (value, _)) in dispatch.arms.iter().enumerate() {
            entries[*value as usize] = body_offsets[source_index];
        }
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset: dispatch.arms.len() as u32 + 1,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn recognizes_source_order_independent_dense_virtual_cases() {
        let function = Function {
            return_type: Type::Void,
            name: "dispatch".into(),
            is_static: false,
            is_weak: true,
            parameters: vec![
                Parameter { parameter_type: Type::StructPointer { element_size: 0 }, name: "this".into() },
                Parameter { parameter_type: Type::StructPointer { element_size: 0 }, name: "target".into() },
                Parameter { parameter_type: Type::StructPointer { element_size: 0 }, name: "msg".into() },
            ],
            locals: Vec::new(),
            statements: vec![Statement::Switch {
                scrutinee: Expression::Member {
                    base: Box::new(Expression::Variable("msg".into())),
                    offset: 4,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                arms: (0..7).rev().map(|value| mwcc_syntax_trees::SwitchArm {
                    value,
                    body: mwcc_syntax_trees::ArmBody::Statements(vec![Statement::Expression(
                        Expression::VirtualCall {
                            object: Box::new(Expression::Variable("this".into())),
                            vptr_offset: 0,
                            slot_offset: 12 + value as u16 * 4,
                            return_type: Type::Void,
                            variadic: false,
                            arguments: vec![Expression::Variable("target".into()), Expression::Variable("msg".into())],
                        },
                    )]),
                    falls_through: false,
                }).collect(),
                default: None,
            }],
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
        assert_eq!(
            recognize(&function),
            Some(DenseVirtualSwitch {
                member_offset: 4,
                arms: (0..7).rev().map(|value| (value, 12 + value as u16 * 4)).collect(),
            })
        );
    }
}
