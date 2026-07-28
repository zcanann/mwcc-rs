//! Guarded list destruction composed with a verified skipped-inline wipe.
//!
//! The helper summary owns the node walk and terminal list stores. This owner
//! composes that transaction into a caller which conditionally frees the list
//! object itself, preserving the measured linkage and register schedule.

#[allow(unused_imports)]
use super::*;

struct InlinedListWipe<'a> {
    helper: &'a str,
    release_callee: &'a str,
    list_global: &'a str,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn negated_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = expression
    else {
        return None;
    };
    let Expression::Call { name, arguments } = operand.as_ref() else {
        return None;
    };
    Some((name, arguments))
}

fn classify(function: &Function) -> Option<InlinedListWipe<'_>> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [list_pointer] = function.parameters.as_slice() else {
        return None;
    };
    if list_pointer.parameter_type != Type::Pointer(Pointee::Pointer) {
        return None;
    }
    let [wipe_guard, release_guard] = function.guards.as_slice() else {
        return None;
    };
    if constant_value(&wipe_guard.value) != Some(0)
        || constant_value(&release_guard.value) != Some(0)
    {
        return None;
    }
    let (helper, wipe_arguments) = negated_call(&wipe_guard.condition)?;
    if !matches!(wipe_arguments, [Expression::Dereference { pointer }]
        if var(pointer, &list_pointer.name))
    {
        return None;
    }
    let (release_callee, release_arguments) = negated_call(&release_guard.condition)?;
    let [Expression::AddressOf { operand: global }, Expression::Cast { operand, .. }] =
        release_arguments
    else {
        return None;
    };
    let Expression::Variable(list_global) = global.as_ref() else {
        return None;
    };
    if !var(operand, &list_pointer.name) {
        return None;
    }
    Some(InlinedListWipe {
        helper,
        release_callee,
        list_global,
    })
}

impl Generator {
    pub(crate) fn try_inlined_list_wipe(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let Some(wipe) = self.inline_summaries.list_wipe(shape.helper).cloned() else {
            return Ok(false);
        };
        if !self.skipped_inline_names.contains(shape.helper)
            || !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }

        const LIST_POINTER: u8 = 29;
        const LIST: u8 = 30;
        const NEXT_NODE: u8 = 31;
        const NODE_SLOT: i16 = 12;
        let loop_condition = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_continue = self.fresh_label();
        let wipe_result = self.fresh_label();
        let release = self.fresh_label();
        let release_success = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![NEXT_NODE, LIST, LIST_POINTER];
        self.output.pre_scheduled = true;
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
                offset: -32,
            },
            Instruction::StoreWord {
                s: NEXT_NODE,
                a: 1,
                offset: 28,
            },
            Instruction::StoreWord {
                s: LIST,
                a: 1,
                offset: 24,
            },
            Instruction::StoreWord {
                s: LIST_POINTER,
                a: 1,
                offset: 20,
            },
            Instruction::move_register(LIST_POINTER, 3),
            Instruction::LoadWord {
                d: LIST,
                a: 3,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: LIST,
                offset: wipe.head_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: NODE_SLOT,
            },
        ]);
        self.emit_branch_to(loop_condition);
        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: NEXT_NODE,
                a: 3,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: NODE_SLOT,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &wipe.free_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: wipe.free_callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_continue);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(wipe_result);
        self.bind_label(loop_continue);
        self.output.instructions.push(Instruction::StoreWord {
            s: NEXT_NODE,
            a: 1,
            offset: NODE_SLOT,
        });
        self.bind_label(loop_condition);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: LIST,
                offset: wipe.count_offset,
            },
            Instruction::load_immediate(3, 1),
            Instruction::StoreWord {
                s: 0,
                a: LIST,
                offset: wipe.next_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: LIST,
                offset: wipe.head_offset,
            },
        ]);
        self.bind_label(wipe_result);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, release);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(release);
        self.record_relocation(RelocationKind::Addr16Ha, shape.list_global);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, shape.list_global);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: LIST_POINTER,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.release_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.release_callee.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, release_success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(release_success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: NEXT_NODE,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: LIST,
                a: 1,
                offset: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord {
                d: LIST_POINTER,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
