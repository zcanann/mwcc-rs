//! Packed-handle lookup followed by a linked-index table walk.
//!
//! Resource archives encode a table number in the high half of a handle and an
//! entry number in the low half.  After validating both halves, the lookup walks
//! fixed-stride entries through an unsigned-short link until it reaches the
//! sentinel.  The high and low halves, archive base, and current entry overlap
//! for almost the entire leaf function, so lowering the statements separately
//! loses MWCC's register ownership and early-return topology.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy)]
struct GlobalPointerTableLinkSearch<'a> {
    count: &'a str,
    table: &'a str,
    entry_stride: u32,
    entry_count_offset: i16,
    link_offset: i16,
}

impl Generator {
    pub(crate) fn try_global_pointer_table_link_search(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(search) = recognize(function) else {
            return Ok(false);
        };
        if self.globals.get(search.count) != Some(&Type::UnsignedInt)
            || !self.globals.contains_key(search.table)
            || !search.entry_stride.is_power_of_two()
            || search.entry_stride > (1 << 31)
        {
            return Ok(false);
        }
        let shift = u8::try_from(search.entry_stride.trailing_zeros())
            .expect("a 32-bit stride has a representable shift");
        let entry_step = i16::try_from(search.entry_stride).map_err(|_| {
            Diagnostic::error("linked pointer-table entry stride does not fit an addi")
        })?;

        self.output.pre_scheduled = true;
        let count_valid = self.fresh_label();
        let archive_valid = self.fresh_label();
        let entry_valid = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();

        // The independent count load leads the extraction packet. MWCC assigns
        // the high handle half to r4 and its low half to r5.
        self.emit_global_load(search.count, 0)?;
        self.output.instructions.extend([
            Instruction::ShiftRightLogicalImmediate {
                a: 4,
                s: 3,
                shift: 16,
            },
            Instruction::ClearLeftImmediate {
                a: 5,
                s: 3,
                clear: 16,
            },
        ]);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(12, 0, count_valid); // blt
        self.emit_null_return();

        self.bind_label(count_valid);
        self.emit_global_load(search.table, 3)?;
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 2,
            },
            Instruction::LoadWordIndexed { d: 4, a: 3, b: 0 },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, archive_valid); // bne
        self.emit_null_return();

        self.bind_label(archive_valid);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: search.entry_count_offset,
            },
            Instruction::CompareLogicalWord { a: 5, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 0, entry_valid); // blt
        self.emit_null_return();

        self.bind_label(entry_valid);
        self.emit_linked_entry_address(5, shift, entry_step);
        self.emit_branch_to(loop_condition);

        self.bind_label(loop_body);
        self.emit_linked_entry_address(0, shift, entry_step);

        self.bind_label(loop_condition);
        self.output.instructions.extend([
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 3,
                offset: search.link_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: u16::MAX,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, loop_body); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_null_return(&mut self) {
        self.output.instructions.extend([
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ]);
    }

    fn emit_linked_entry_address(&mut self, index: u8, shift: u8, entry_step: i16) {
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 3,
                s: index,
                shift,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: entry_step,
            },
            Instruction::Add { d: 3, a: 4, b: 3 },
        ]);
    }
}

fn recognize(function: &Function) -> Option<GlobalPointerTableLinkSearch<'_>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let Type::StructPointer { element_size: return_stride } = function.return_type else {
        return None;
    };
    let [low, archive, entry] = function.locals.as_slice() else {
        return None;
    };
    if parameter.parameter_type != Type::UnsignedInt
        || low.declared_type != Type::UnsignedInt
        || archive.declared_type != function.return_type
        || entry.declared_type != function.return_type
        || low.is_static
        || archive.is_static
        || entry.is_static
        || archive.initializer.is_some()
        || entry.initializer.is_some()
        || !function.guards.is_empty()
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &entry.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: masked_parameter,
        right: mask,
    } = low.initializer.as_ref()?
    else {
        return None;
    };
    if !variable(masked_parameter, &parameter.name) || constant_value(mask) != Some(0xffff) {
        return None;
    }

    let [count_guard, archive_assignment, null_guard, entry_guard, entry_assignment, walk] =
        function.statements.as_slice()
    else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: high,
                right: count_expression,
            },
        then_body: count_failure,
        else_body: count_else,
    } = count_guard
    else {
        return None;
    };
    let Expression::Variable(count) = count_expression.as_ref() else {
        return None;
    };
    if !high_half(high, &parameter.name)
        || !null_return(count_failure)
        || !count_else.is_empty()
    {
        return None;
    }

    let Statement::Assign {
        name: archive_name,
        value:
            Expression::Index {
                base: table_expression,
                index: archive_index,
            },
    } = archive_assignment
    else {
        return None;
    };
    let Expression::Variable(table) = table_expression.as_ref() else {
        return None;
    };
    if archive_name != &archive.name || !high_half(archive_index, &parameter.name) {
        return None;
    }
    if !matches!(null_guard,
        Statement::If {
            condition: Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            },
            then_body,
            else_body,
        } if variable(operand, &archive.name)
            && null_return(then_body)
            && else_body.is_empty())
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: guarded_low,
                right: entry_count,
            },
        then_body: entry_failure,
        else_body: entry_else,
    } = entry_guard
    else {
        return None;
    };
    let Expression::Member {
        base: count_base,
        offset: entry_count_offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = entry_count.as_ref()
    else {
        return None;
    };
    if !variable(guarded_low, &low.name)
        || !variable(count_base, &archive.name)
        || !null_return(entry_failure)
        || !entry_else.is_empty()
    {
        return None;
    }

    let Statement::Assign {
        name: entry_name,
        value: initial_entry,
    } = entry_assignment
    else {
        return None;
    };
    if entry_name != &entry.name
        || !entry_pointer_index(initial_entry, &archive.name, return_stride)
            .is_some_and(|index| variable(index, &low.name))
    {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition:
            Some(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: link,
                right: sentinel,
            }),
        step: None,
        body,
    } = walk
    else {
        return None;
    };
    let Expression::Member {
        base: link_base,
        offset: link_offset,
        member_type: Type::UnsignedShort,
        index_stride: None,
    } = link.as_ref()
    else {
        return None;
    };
    let [Statement::Assign {
        name: walked_entry,
        value: next_entry,
    }] = body.as_slice()
    else {
        return None;
    };
    if !variable(link_base, &entry.name)
        || constant_value(sentinel) != Some(0xffff)
        || walked_entry != &entry.name
        || !entry_pointer_index(next_entry, &archive.name, return_stride).is_some_and(|index| {
            matches!(index,
                Expression::Member {
                    base,
                    offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                } if variable(base, &entry.name) && offset == link_offset)
        })
    {
        return None;
    }

    Some(GlobalPointerTableLinkSearch {
        count,
        table,
        entry_stride: return_stride,
        entry_count_offset: i16::try_from(*entry_count_offset).ok()?,
        link_offset: i16::try_from(*link_offset).ok()?,
    })
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(actual) if actual == name)
}

fn high_half(expression: &Expression, parameter: &str) -> bool {
    matches!(expression,
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } if variable(left, parameter) && constant_value(right) == Some(16))
}

fn null_return(statements: &[Statement]) -> bool {
    matches!(statements, [Statement::Return(Some(value))] if constant_value(value) == Some(0))
}

fn entry_pointer_index<'a>(
    expression: &'a Expression,
    base: &str,
    stride: u32,
) -> Option<&'a Expression> {
    let Expression::Cast {
        target_type: Type::StructPointer { element_size },
        operand,
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = operand.as_ref()
    else {
        return None;
    };
    let Expression::AddressOf { operand } = left.as_ref() else {
        return None;
    };
    let Expression::Index {
        base: indexed_base,
        index,
    } = operand.as_ref()
    else {
        return None;
    };
    (*element_size == stride
        && constant_value(right) == Some(1)
        && variable(indexed_base, base))
    .then_some(index.as_ref())
}
