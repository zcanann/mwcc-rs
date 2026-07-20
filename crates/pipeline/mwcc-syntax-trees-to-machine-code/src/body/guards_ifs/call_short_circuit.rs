//! Short-circuit guards whose terms are calls sharing one live float input.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `if (!left(y) && !right(y)) return T; return F;` preserves `y` in f31,
    /// short-circuits on either nonzero call result, and shares one epilogue.
    /// This is the canonical GC/3.x lowering measured in Twilight Princess.
    pub(crate) fn try_float_call_short_circuit_guard(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || matches!(function.return_type, Type::Void | Type::Float | Type::Double)
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::Float {
            return Ok(false);
        }
        let [guard] = function.guards.as_slice() else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } = &guard.condition
        else {
            return Ok(false);
        };

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

        let Some((left_name, left_arguments)) = negated_call(left) else {
            return Ok(false);
        };
        let Some((right_name, right_arguments)) = negated_call(right) else {
            return Ok(false);
        };
        let argument_is_parameter = |arguments: &[Expression]| {
            matches!(arguments, [Expression::Variable(name)] if name == &parameter.name)
        };
        if !argument_is_parameter(left_arguments)
            || !argument_is_parameter(right_arguments)
            || self.locations.contains_key(left_name)
            || self.locations.contains_key(right_name)
            || self.globals.contains_key(left_name)
            || self.globals.contains_key(right_name)
        {
            return Ok(false);
        }
        let Some(true_value) = constant_value(&guard.value) else {
            return Ok(false);
        };
        let Some(false_value) = function.return_expression.as_ref().and_then(constant_value) else {
            return Ok(false);
        };
        if i16::try_from(true_value).is_err() || i16::try_from(false_value).is_err() {
            return Ok(false);
        }

        self.non_leaf = true;
        self.callee_saved_float = 1;
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 31,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 31, b: 1 });

        self.record_relocation(RelocationKind::Rel24, left_name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: left_name.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let first_false_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });

        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 31 });
        self.record_relocation(RelocationKind::Rel24, right_name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: right_name.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let second_false_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.load_integer_constant(3, true_value);
        let epilogue_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let false_block = self.output.instructions.len();
        self.patch_forward(first_false_branch, false_block);
        self.patch_forward(second_false_branch, false_block);
        self.load_integer_constant(3, false_value);

        let epilogue = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[epilogue_branch] {
            *target = epilogue;
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatDouble {
                d: 31,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}
