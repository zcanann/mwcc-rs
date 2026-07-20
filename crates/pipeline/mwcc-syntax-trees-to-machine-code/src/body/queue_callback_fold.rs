//! Callback-queue traversal with a folded failure accumulator.

#[allow(unused_imports)]
use super::*;

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn accumulated_not<'a>(statement: &'a Statement, accumulator: &str) -> Option<&'a Expression> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = right.as_ref()
    else {
        return None;
    };
    (name == accumulator && variable(left, accumulator)).then_some(operand)
}

impl Generator {
    pub(crate) fn try_queue_callback_fold(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Int
            || function.parameters.len() != 1
            || function.parameters[0].parameter_type != Type::Int
            || function.locals.len() != 2
            || function.statements.len() != 2
            || function.guards.len() != 1
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let argument = &function.parameters[0].name;
        let iterator = &function.locals[0].name;
        let accumulator = &function.locals[1].name;
        if !matches!(function.locals[0].declared_type, Type::StructPointer { .. })
            || function.locals[1].declared_type != Type::Int
            || function.locals[1]
                .initializer
                .as_ref()
                .and_then(constant_value)
                != Some(0)
            || !matches!(function.return_expression.as_ref(), Some(value) if constant_value(value) == Some(1))
            || !variable(&function.guards[0].condition, accumulator)
            || constant_value(&function.guards[0].value) != Some(0)
        {
            return Ok(false);
        }
        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign { target, value }),
            condition: Some(condition),
            step: Some(Expression::Assign { target: step_target, value: step_value }),
            body,
        } = &function.statements[0]
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: queue_base,
            offset: 0,
            index_stride: None,
            ..
        } = value.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(queue) = queue_base.as_ref() else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: condition_left,
            right: condition_right,
        } = condition
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: next_base,
            offset: next_offset,
            index_stride: None,
            ..
        } = step_value.as_ref()
        else {
            return Ok(false);
        };
        let [body_statement] = body.as_slice() else {
            return Ok(false);
        };
        let Some(indirect) = accumulated_not(body_statement, accumulator) else {
            return Ok(false);
        };
        let Expression::CallThrough {
            target: callback_target,
            arguments,
        } = indirect
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: callback_base,
            offset: callback_offset,
            index_stride: None,
            ..
        } = callback_target.as_ref()
        else {
            return Ok(false);
        };
        let Some(direct) = accumulated_not(&function.statements[1], accumulator) else {
            return Ok(false);
        };
        let Expression::Call {
            name: sync,
            arguments: sync_arguments,
        } = direct
        else {
            return Ok(false);
        };
        if !variable(target, iterator)
            || !variable(step_target, iterator)
            || !variable(next_base, iterator)
            || !variable(condition_left, iterator)
            || constant_value(condition_right).or_else(|| match condition_right.as_ref() {
                Expression::Cast { operand, .. } => constant_value(operand),
                _ => None,
            }) != Some(0)
            || !variable(callback_base, iterator)
            || !matches!(arguments.as_slice(), [value] if variable(value, argument))
            || !sync_arguments.is_empty()
            || !self.globals.contains_key(queue.as_str())
        {
            return Ok(false);
        }
        let (Ok(next), Ok(callback)) =
            (i16::try_from(*next_offset), i16::try_from(*callback_offset))
        else {
            return Ok(false);
        };

        self.output.pre_scheduled = true;
        // The loop and final return diamond consume seven internal compiler labels before the
        // function's extab symbol, even though only the resolved branches survive in `.text`.
        self.output.anonymous_label_bump += 7;
        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31, 30, 29];
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 4 });
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::StoreWord { s: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::StoreWord { s: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::load_immediate(30, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 29, a: 3, immediate: 0 });
        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 0, offset: 0 });

        let body_label = self.fresh_label();
        let condition_label = self.fresh_label();
        self.emit_branch_to(condition_label);
        self.bind_label(body_label);
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: 31, offset: callback });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 29, immediate: 0 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToLinkRegisterAndLink);
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 31, offset: next });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::Or { a: 30, s: 30, b: 0 });
        self.bind_label(condition_label);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 31, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, body_label);

        self.record_relocation(RelocationKind::Rel24, sync);
        self.output.instructions.push(Instruction::BranchAndLink { target: sync.clone() });
        self.output.instructions.push(Instruction::CountLeadingZeros { a: 0, s: 3 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 5 });
        self.output.instructions.push(Instruction::OrRecord { a: 30, s: 30, b: 0 });
        let success = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, success);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(join);
        self.bind_label(success);
        self.output.instructions.push(Instruction::load_immediate(3, 1));
        self.bind_label(join);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::LoadWord { d: 31, a: 1, offset: 28 });
        self.output.instructions.push(Instruction::LoadWord { d: 30, a: 1, offset: 24 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::LoadWord { d: 29, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
