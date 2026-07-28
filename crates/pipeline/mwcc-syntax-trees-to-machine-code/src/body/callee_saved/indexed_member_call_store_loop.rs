//! Fixed-count struct walks that feed one member through a call into another.
//!
//! The element address and loop counter both cross the call. GC/2.x advances
//! the element pointer in a callee-saved register instead of rebuilding
//! `base + index * stride` on either side of the call.

#[allow(unused_imports)]
use super::*;

struct IndexedMemberCallStoreLoop<'a> {
    callee: &'a str,
    source_offset: i16,
    target_offset: i16,
    stride: i16,
    bound: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn indexed_member<'a>(
    expression: &'a Expression,
    parameter: &str,
    index: &str,
) -> Option<(u32, Type, u32)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: Some(stride),
    } = expression
    else {
        return None;
    };
    let Expression::Index {
        base: indexed_base,
        index: indexed_index,
    } = base.as_ref()
    else {
        return None;
    };
    (variable(indexed_base, parameter) && variable(indexed_index, index)).then_some((
        *offset,
        *member_type,
        *stride,
    ))
}

fn classify(function: &Function) -> Option<IndexedMemberCallStoreLoop<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let Type::StructPointer { element_size } = parameter.parameter_type else {
        return None;
    };
    let [index] = function.locals.as_slice() else {
        return None;
    };
    if index.declared_type != Type::Int
        || index.initializer.is_some()
        || index.array_length.is_some()
        || index.is_static
        || index.is_volatile
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer,
        Expression::Assign { target, value }
            if variable(target, &index.name) && constant_value(value) == Some(0))
        || !matches!(step,
            Expression::Assign { target, value }
                if variable(target, &index.name)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left, &index.name) => i16::try_from(constant_value(right)?)
            .ok()
            .filter(|value| *value > 0)?,
        _ => return None,
    };
    let [Statement::Store {
        target,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    }] = body.as_slice()
    else {
        return None;
    };
    let [argument] = arguments.as_slice() else {
        return None;
    };
    let (target_offset, target_type, target_stride) =
        indexed_member(target, &parameter.name, &index.name)?;
    let (source_offset, source_type, source_stride) =
        indexed_member(argument, &parameter.name, &index.name)?;
    if target_stride != element_size
        || source_stride != element_size
        || !matches!(
            target_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_)
        )
        || !matches!(
            source_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }
    Some(IndexedMemberCallStoreLoop {
        callee,
        source_offset: i16::try_from(source_offset).ok()?,
        target_offset: i16::try_from(target_offset).ok()?,
        stride: i16::try_from(element_size)
            .ok()
            .filter(|value| *value > 0)?,
        bound,
    })
}

impl Generator {
    pub(crate) fn try_indexed_member_call_store_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        const CURSOR: u8 = 31;
        const COUNTER: u8 = 30;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![CURSOR, COUNTER];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: CURSOR,
                a: 1,
                offset: 12,
            },
            Instruction::move_register(CURSOR, 3),
            Instruction::StoreWord {
                s: COUNTER,
                a: 1,
                offset: 8,
            },
            Instruction::load_immediate(COUNTER, 0),
        ]);

        let loop_top = self.output.instructions.len();
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: CURSOR,
            offset: shape.source_offset,
        });
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: COUNTER,
                a: COUNTER,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 3,
                a: CURSOR,
                offset: shape.target_offset,
            },
            Instruction::CompareWordImmediate {
                a: COUNTER,
                immediate: shape.bound,
            },
            Instruction::AddImmediate {
                d: CURSOR,
                a: CURSOR,
                immediate: shape.stride,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_top,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: CURSOR,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: COUNTER,
                a: 1,
                offset: 8,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 1;
        Ok(true)
    }
}
