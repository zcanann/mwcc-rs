//! Paired EABI linker-table initialization loops.
//!
//! Startup code walks a three-word ROM-copy table and then a two-word BSS table.
//! Both tiny helpers are automatically inlined, so their cursors, call arguments,
//! branches, and saved registers form one optimizer transaction. Lowering either
//! loop independently loses build 163's retained cursor lane and branch schedule.

#[allow(unused_imports)]
use super::*;

struct LinkerInitializationPlan {
    rom_table: String,
    bss_table: String,
    copy: String,
    flush: String,
    clear: String,
}

impl Generator {
    pub(crate) fn try_linker_table_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !function.preceded_by_asm
            || function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || function.section.as_deref() != Some(".init")
        {
            return Ok(false);
        }
        let Some(plan) = recognize(function, &self.inline_bodies) else {
            return Ok(false);
        };

        self.emit_linker_table_initialization(plan);
        Ok(true)
    }

    fn emit_linker_table_initialization(&mut self, plan: LinkerInitializationPlan) {
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![31, 30, 29];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 12,
            },
        ]);

        self.emit_table_address(&plan.rom_table);
        let rom_entry = self.fresh_label();
        let rom_loop = self.fresh_label();
        let bss_start = self.fresh_label();
        let rom_next = self.fresh_label();
        self.emit_branch_to(rom_entry);
        self.bind_label(rom_entry);
        self.emit_branch_to(rom_loop);
        self.bind_label(rom_loop);
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 29,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 30,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, bss_start);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 29,
            offset: 4,
        });
        // The inlined helper retains its redundant `size == 0` test and reuses
        // the CR set by the loop sentinel check.
        self.emit_branch_conditional_to(12, 2, rom_next);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 31, b: 4 });
        self.emit_branch_conditional_to(12, 2, rom_next);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::move_register(5, 30));
        self.emit_linker_call(&plan.copy);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.emit_linker_call(&plan.flush);
        self.bind_label(rom_next);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 12,
        });
        self.emit_branch_to(rom_loop);

        self.bind_label(bss_start);
        self.emit_table_address(&plan.bss_table);
        let bss_entry = self.fresh_label();
        let bss_loop = self.fresh_label();
        let bss_next = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_to(bss_entry);
        self.bind_label(bss_entry);
        self.emit_branch_to(bss_loop);
        self.bind_label(bss_loop);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 29,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 5,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, epilogue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 0,
        });
        self.emit_branch_conditional_to(12, 2, bss_next);
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.emit_linker_call(&plan.clear);
        self.bind_label(bss_next);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 29,
            immediate: 8,
        });
        self.emit_branch_to(bss_loop);

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 16,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord {
                d: 29,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }

    fn emit_table_address(&mut self, table: &str) {
        self.emit_address_high(3, table);
        self.record_relocation(RelocationKind::Addr16Lo, table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 0));
    }

    fn emit_linker_call(&mut self, callee: &str) {
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.to_owned(),
        });
    }
}

fn recognize(
    function: &Function,
    inline_bodies: &crate::inline_expansion::InlineBodySet,
) -> Option<LinkerInitializationPlan> {
    let [rom_cursor, bss_cursor] = function.locals.as_slice() else {
        return None;
    };
    if rom_cursor.declared_type != (Type::StructPointer { element_size: 12 })
        || bss_cursor.declared_type != (Type::StructPointer { element_size: 8 })
    {
        return None;
    }
    let [
        Statement::Assign {
            name: rom_name,
            value: Expression::Variable(rom_table),
        },
        rom_loop,
        Statement::Assign {
            name: bss_name,
            value: Expression::Variable(bss_table),
        },
        bss_loop,
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if rom_name != &rom_cursor.name || bss_name != &bss_cursor.name {
        return None;
    }
    let copy_helper = recognize_table_loop(rom_loop, rom_name, 8, &[4, 0, 8])?;
    let clear_helper = recognize_table_loop(bss_loop, bss_name, 4, &[0, 4])?;
    let copy_body = inline_bodies
        .definition_body(copy_helper)
        .or_else(|| inline_bodies.composable_body(copy_helper))?;
    let clear_body = inline_bodies
        .definition_body(clear_helper)
        .or_else(|| inline_bodies.composable_body(clear_helper))?;
    let (copy, flush) = recognize_copy_helper(copy_body)?;
    let clear = recognize_clear_helper(clear_body)?;

    Some(LinkerInitializationPlan {
        rom_table: rom_table.clone(),
        bss_table: bss_table.clone(),
        copy: copy.to_owned(),
        flush: flush.to_owned(),
        clear: clear.to_owned(),
    })
}

fn recognize_table_loop<'a>(
    statement: &'a Statement,
    cursor: &str,
    size_offset: u32,
    call_offsets: &[u32],
) -> Option<&'a str> {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(Expression::IntegerLiteral(1)),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    let [
        Statement::If {
            condition,
            then_body,
            else_body,
        },
        Statement::Expression(Expression::Call { name: callee, arguments }),
        Statement::Assign { name, value },
    ] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Break])
        || !is_zero_member_test(condition, cursor, size_offset)
        || name != cursor
        || !is_cursor_step(value, cursor)
        || arguments.len() != call_offsets.len()
        || !arguments
            .iter()
            .zip(call_offsets)
            .all(|(argument, &offset)| is_member(argument, cursor, offset))
    {
        return None;
    }
    Some(callee)
}

fn recognize_copy_helper(function: &Function) -> Option<(&str, &str)> {
    let [destination, source, size] = function.parameters.as_slice() else {
        return None;
    };
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: copy,
        arguments: copy_arguments,
    }), Statement::Expression(Expression::Call {
        name: flush,
        arguments: flush_arguments,
    })] = then_body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !is_variable(left, &size.name)
        || !matches!(
            right.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if is_variable(left, &destination.name) && is_variable(right, &source.name)
        )
        || !matches!(
            copy_arguments.as_slice(),
            [a, b, c]
                if is_variable(a, &destination.name)
                    && is_variable(b, &source.name)
                    && is_variable(c, &size.name)
        )
        || !matches!(
            flush_arguments.as_slice(),
            [a, b] if is_variable(a, &destination.name) && is_variable(b, &size.name)
        )
    {
        return None;
    }
    Some((copy, flush))
}

fn recognize_clear_helper(function: &Function) -> Option<&str> {
    let [destination, size] = function.parameters.as_slice() else {
        return None;
    };
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: clear,
        arguments,
    })] = then_body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !is_variable(condition, &size.name)
        || !matches!(
            arguments.as_slice(),
            [a, Expression::IntegerLiteral(0), c]
                if is_variable(a, &destination.name) && is_variable(c, &size.name)
        )
    {
        return None;
    }
    Some(clear)
}

fn is_zero_member_test(expression: &Expression, cursor: &str, offset: u32) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if is_member(left, cursor, offset) && constant_value(right) == Some(0)
    )
}

fn is_cursor_step(expression: &Expression, cursor: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if is_variable(left, cursor) && constant_value(right) == Some(1)
    )
}

fn is_member(expression: &Expression, cursor: &str, offset: u32) -> bool {
    matches!(
        expression,
        Expression::Member { base, offset: actual, .. }
            if *actual == offset && is_variable(base, cursor)
    )
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(actual) if actual == name)
}
