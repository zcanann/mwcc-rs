//! Display-list cursor padding to the next 32-byte boundary.
//!
//! Dolphin's `GDPadCurr32` expands a one-byte write helper inside a counted
//! loop. MWCC turns the remaining-byte count into an eight-way unrolled CTR
//! loop plus a scalar CTR tail. Keep recognition and emission together here:
//! generic structured-loop lowering does not yet represent an inline-expanded
//! post-incremented member pointer with this unroll schedule.

use super::*;

struct PaddingLoop<'a> {
    cursor_global: &'a str,
}

impl Generator {
    pub(crate) fn try_display_list_padding_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if !matches!(
            self.globals.get(shape.cursor_global),
            Some(Type::StructPointer { .. } | Type::Pointer(_))
        ) {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![shape.cursor_global.to_string()];

        let unrolled = self.fresh_label();
        let tail = self.fresh_label();
        let tail_loop = self.fresh_label();

        self.record_relocation(RelocationKind::EmbSda21, shape.cursor_global);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 8,
            },
            Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 27,
            },
            Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 32,
            },
            Instruction::SubtractFromImmediate {
                d: 3,
                a: 0,
                immediate: 32,
            },
            Instruction::load_immediate(6, 0),
            Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 3,
                shift: 29,
                begin: 3,
                end: 31,
            },
            Instruction::MoveToCountRegister { s: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, tail);

        self.bind_label(unrolled);
        for _ in 0..8 {
            self.emit_display_list_padding_byte(shape.cursor_global);
        }
        self.emit_branch_conditional_to(16, 0, unrolled);
        self.output
            .instructions
            .push(Instruction::AndImmediateRecord {
                a: 3,
                s: 3,
                immediate: 7,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });

        self.bind_label(tail);
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 3 });
        self.bind_label(tail_loop);
        self.emit_display_list_padding_byte(shape.cursor_global);
        self.emit_branch_conditional_to(16, 0, tail_loop);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_display_list_padding_byte(&mut self, cursor_global: &str) {
        self.record_relocation(RelocationKind::EmbSda21, cursor_global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 5,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 8,
            },
            Instruction::StoreByte {
                s: 6,
                a: 4,
                offset: 0,
            },
        ]);
    }
}

fn recognize(function: &Function) -> Option<PaddingLoop<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if local.declared_type != Type::UnsignedInt
        || local.is_static
        || local.array_length.is_some()
        || local.is_volatile
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = local.initializer.as_ref()?
    else {
        return None;
    };
    if constant_value(right)? != 31 {
        return None;
    }
    let Expression::Cast {
        target_type: Type::UnsignedInt,
        operand,
    } = left.as_ref()
    else {
        return None;
    };
    let cursor_global = byte_cursor_member(operand)?;

    let [Statement::If {
        condition,
        then_body,
        else_body,
        ..
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !is_variable(condition, &local.name) || !else_body.is_empty() {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer,
        condition,
        step,
        body,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !initializer
        .as_ref()
        .is_some_and(|expression| is_variable(expression, &local.name))
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition.as_ref()?
    else {
        return None;
    };
    if !is_variable(left, &local.name) || constant_value(right)? != 32 {
        return None;
    }
    let Expression::Assign { target, value } = step.as_ref()? else {
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
    if !is_variable(target, &local.name)
        || !is_variable(left, &local.name)
        || constant_value(right)? != 1
    {
        return None;
    }
    let [Statement::Store { target, value }] = body.as_slice() else {
        return None;
    };
    let Expression::Dereference { pointer } = target else {
        return None;
    };
    let Expression::PostStep {
        target,
        operator: BinaryOperator::Add,
        pointer_link: None,
    } = pointer.as_ref()
    else {
        return None;
    };
    if byte_cursor_member(target)? != cursor_global || constant_value(value)? != 0 {
        return None;
    }

    Some(PaddingLoop { cursor_global })
}

fn byte_cursor_member(expression: &Expression) -> Option<&str> {
    let Expression::Member {
        base,
        offset: 8,
        member_type: Type::Pointer(Pointee::UnsignedChar),
        index_stride: None,
    } = expression
    else {
        return None;
    };
    match base.as_ref() {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn is_variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}
