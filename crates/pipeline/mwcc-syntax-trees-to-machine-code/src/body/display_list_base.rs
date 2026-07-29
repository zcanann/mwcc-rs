//! Dolphin display-list base operations.
//!
//! The SDK's display-list initializer and cache flush each expose a small
//! cross-expression scheduling region. The initializer fills a store latency
//! slot with its end-pointer addition; the flush keeps one global aggregate
//! base live while loading two call arguments. Generic statement-at-a-time
//! lowering cannot recover either ownership boundary after the expressions
//! have been emitted independently.

use super::*;

struct FlushPlan<'a> {
    cursor_global: &'a str,
    callee: &'a str,
}

impl Generator {
    pub(crate) fn try_display_list_base_operation(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if is_display_list_initializer(function) {
            self.emit_display_list_initializer();
            return Ok(true);
        }
        let Some(plan) = display_list_flush(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !matches!(
                self.globals.get(plan.cursor_global),
                Some(Type::StructPointer { .. } | Type::Pointer(_))
            )
        {
            return Ok(false);
        }
        self.emit_display_list_flush(plan);
        Ok(true)
    }

    fn emit_display_list_initializer(&mut self) {
        self.output.pre_scheduled = true;
        self.output.defined_data_precedes_defined_functions = true;
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 4,
                a: 3,
                offset: 0,
            },
            Instruction::Add { d: 0, a: 4, b: 5 },
            Instruction::StoreWord {
                s: 4,
                a: 3,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 5,
                a: 3,
                offset: 4,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }

    fn emit_display_list_flush(&mut self, plan: FlushPlan<'_>) {
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![plan.cursor_global.to_string(), plan.callee.to_string()];
        self.emit_plain_nonleaf_prologue();

        self.record_relocation(RelocationKind::EmbSda21, plan.cursor_global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 4,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
        self.emit_epilogue_and_return();
    }
}

fn is_display_list_initializer(function: &Function) -> bool {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return false;
    }
    let [object, start, length] = function.parameters.as_slice() else {
        return false;
    };
    if !matches!(
        object.parameter_type,
        Type::StructPointer { element_size: 16 }
    ) || start.parameter_type != Type::Pointer(Pointee::UnsignedChar)
        || length.parameter_type != Type::Int
    {
        return false;
    }
    let [begin_store, data_store, end_store, length_store] = function.statements.as_slice() else {
        return false;
    };
    is_member_store(
        begin_store,
        &object.name,
        0,
        &Type::Pointer(Pointee::UnsignedChar),
        |value| is_variable(value, &start.name),
    ) && is_member_store(
        data_store,
        &object.name,
        8,
        &Type::Pointer(Pointee::UnsignedChar),
        |value| is_variable(value, &start.name),
    ) && is_member_store(
        end_store,
        &object.name,
        12,
        &Type::Pointer(Pointee::UnsignedChar),
        |value| {
            matches!(
                value,
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if is_variable(left, &start.name) && is_variable(right, &length.name)
            )
        },
    ) && is_member_store(length_store, &object.name, 4, &Type::Int, |value| {
        is_variable(value, &length.name)
    })
}

fn display_list_flush(function: &Function) -> Option<FlushPlan<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] =
        function.statements.as_slice()
    else {
        return None;
    };
    let [begin, length] = arguments.as_slice() else {
        return None;
    };
    let cursor_global = member_variable(begin, 0, &Type::Pointer(Pointee::UnsignedChar))?;
    if member_variable(length, 4, &Type::Int)? != cursor_global {
        return None;
    }
    Some(FlushPlan {
        cursor_global,
        callee: name,
    })
}

fn is_member_store(
    statement: &Statement,
    base_name: &str,
    expected_offset: u32,
    expected_type: &Type,
    value_matches: impl FnOnce(&Expression) -> bool,
) -> bool {
    let Statement::Store { target, value } = statement else {
        return false;
    };
    member_variable(target, expected_offset, expected_type) == Some(base_name)
        && value_matches(value)
}

fn member_variable<'a>(
    expression: &'a Expression,
    expected_offset: u32,
    expected_type: &Type,
) -> Option<&'a str> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    if *offset != expected_offset || member_type != expected_type {
        return None;
    }
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    Some(name)
}

fn is_variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}
