//! Call-result stores followed by a guarded update in one global aggregate.
//!
//! The legacy allocator keeps both the aggregate base and the address of the
//! guarded field in callee-saved registers. Keeping recognition here, beside
//! the other aggregate/call families, prevents the body driver from acquiring
//! another shape-specific schedule.

#[allow(unused_imports)]
use super::*;

fn member(expression: &Expression, global: &str) -> Option<(u16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == global)
        .then(|| Some((u16::try_from(*offset).ok()?, *member_type)))?
}

impl Generator {
    /// Lower
    /// `g.status = call(g.bytes + g.offset, g.offset, LIMIT - g.offset);`
    /// `if (g.status) g.offset = LIMIT;`.
    ///
    /// GC/1.2.5n assigns the aggregate base to r31 and the offset-field address
    /// to r30. The call arguments reuse one offset load, then the guarded store
    /// uses r30 without rebuilding the address after the call.
    pub(crate) fn try_global_call_result_guard(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }

        let [Statement::Store {
            target: status_target,
            value:
                Expression::Call {
                    name: callee,
                    arguments,
                },
        }, Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: status_base,
            offset: status_offset,
            member_type: Type::Int,
            index_stride: None,
        } = status_target
        else {
            return Ok(false);
        };
        let Expression::Variable(global) = status_base.as_ref() else {
            return Ok(false);
        };
        if !matches!(self.globals.get(global.as_str()), Some(Type::Struct { size, .. }) if *size > 8)
            || member(condition, global) != u16::try_from(*status_offset).ok().map(|offset| (offset, Type::Int))
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        let [Statement::Store {
            target: offset_target,
            value: guarded_value,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        let Some((offset_field, offset_type)) = member(offset_target, global) else {
            return Ok(false);
        };
        if !matches!(offset_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(limit) = constant_value(guarded_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let [Expression::Binary {
            operator: BinaryOperator::Add,
            left: bytes,
            right: first_offset,
        }, second_offset, Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: subtract_limit,
            right: third_offset,
        }] = arguments.as_slice()
        else {
            return Ok(false);
        };
        let Expression::MemberAddress {
            base: bytes_base,
            offset: bytes_offset,
            ..
        } = bytes.as_ref()
        else {
            return Ok(false);
        };
        if *bytes_offset != 0
            || !matches!(bytes_base.as_ref(), Expression::Variable(name) if name == global)
            || member(first_offset, global).map(|item| item.0) != Some(offset_field)
            || member(second_offset, global).map(|item| item.0) != Some(offset_field)
            || member(third_offset, global).map(|item| item.0) != Some(offset_field)
            || constant_value(subtract_limit) != Some(i64::from(limit))
        {
            return Ok(false);
        }
        let (status_offset, offset_field) = match (
            i16::try_from(*status_offset),
            i16::try_from(offset_field),
        ) {
            (Ok(status), Ok(offset)) => (status, offset),
            _ => return Ok(false),
        };

        self.non_leaf = true;
        self.callee_saved = vec![31, 30];
        self.frame_size = 24;
        self.output.pre_scheduled = true;

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        // This schedule materializes the start of the full BSS section rather
        // than the named aggregate. The object writer owns creation of the
        // corresponding zero-offset local anchor.
        const BSS_ANCHOR: &str = "...bss.0";
        self.emit_address_high(3, BSS_ANCHOR);
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
                offset: -24,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        });
        self.record_relocation(RelocationKind::Addr16Lo, BSS_ANCHOR);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 31,
            immediate: offset_field,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: offset_field,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 3, a: 31, b: 4 });
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 5,
                a: 4,
                immediate: limit,
            });
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: status_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: status_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done);
        self.output
            .instructions
            .push(Instruction::AddImmediate { d: 0, a: 0, immediate: limit });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.bind_label(done);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediate { d: 1, a: 1, immediate: 24 });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 5;
        Ok(true)
    }
}
