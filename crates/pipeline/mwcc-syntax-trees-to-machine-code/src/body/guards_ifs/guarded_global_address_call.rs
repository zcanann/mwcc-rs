//! Guard a constructor-like call that receives one global's address.
//!
//! Legacy linkage-first builds materialize the small-data address before the
//! frame update, schedule the constant second argument around the LR save, and
//! merge the boolean result through one epilogue.

#[allow(unused_imports)]
use super::*;

struct GuardedGlobalAddressCall {
    global: String,
    callee: String,
    argument: i16,
}

fn classify(function: &Function) -> Option<GuardedGlobalAddressCall> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [guard] = function.guards.as_slice() else {
        return None;
    };
    if constant_value(&guard.value) != Some(0) {
        return None;
    }
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = &guard.condition
    else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = operand.as_ref()
    else {
        return None;
    };
    let [Expression::AddressOf { operand: global }, argument] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(global) = global.as_ref() else {
        return None;
    };
    Some(GuardedGlobalAddressCall {
        global: global.clone(),
        callee: callee.clone(),
        argument: i16::try_from(constant_value(argument)?).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_guarded_global_address_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !matches!(
            self.globals.get(&shape.global),
            Some(Type::Pointer(_) | Type::StructPointer { .. })
        ) || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }

        let success = self.fresh_label();
        let epilogue = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 8;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::EmbSda21, &shape.global);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate(4, shape.argument),
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 8,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
