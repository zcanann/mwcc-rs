//! Guarded virtual forwarding wrappers.
//!
//! These six-argument member wrappers validate one argument through a direct
//! predicate call, then forward the unchanged argument vector through a known
//! primary-vtable slot. The predicate call forces all incoming registers into
//! callee-saved homes before dispatch.

#[allow(unused_imports)]
use super::*;

struct GuardedVirtualForwarder {
    predicate: String,
    vptr_offset: u16,
    slot_offset: u16,
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(variable) if variable == name)
}

fn classify(function: &Function) -> Option<GuardedVirtualForwarder> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || function.parameters.len() != 6
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || function.guards.len() != 1
        || !matches!(function.return_expression, Some(Expression::IntegerLiteral(0)))
    {
        return None;
    }
    let guard = &function.guards[0];
    let Expression::Call {
        name: predicate,
        arguments: predicate_arguments,
    } = &guard.condition
    else {
        return None;
    };
    if predicate_arguments.as_slice().len() != 1
        || !is_variable(&predicate_arguments[0], &function.parameters[1].name)
    {
        return None;
    }
    let Expression::VirtualCall {
        object,
        vptr_offset,
        slot_offset,
        return_type,
        variadic: false,
        arguments,
    } = &guard.value
    else {
        return None;
    };
    if !matches!(return_type, Type::Int | Type::UnsignedInt)
        || !is_variable(object, &function.parameters[0].name)
        || arguments.len() != 5
        || !arguments
            .iter()
            .zip(&function.parameters[1..])
            .all(|(argument, parameter)| is_variable(argument, &parameter.name))
    {
        return None;
    }
    Some(GuardedVirtualForwarder {
        predicate: predicate.clone(),
        vptr_offset: *vptr_offset,
        slot_offset: *slot_offset,
    })
}

impl Generator {
    pub(crate) fn try_guarded_virtual_forwarder(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if !function
            .parameters
            .iter()
            .zip(3u8..=8)
            .all(|(parameter, register)| {
                self.locations
                    .get(&parameter.name)
                    .is_some_and(|location| location.register == register)
            })
        {
            return Ok(false);
        }

        let false_result = self.fresh_label();
        let done = self.fresh_label();
        self.output.pre_scheduled = true;
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = (26..=31).collect();
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 8,
            });
        for (home, incoming) in [(27, 4), (26, 3), (28, 5), (29, 6), (30, 7), (31, 8)] {
            self.output
                .instructions
                .push(Instruction::move_register(home, incoming));
        }
        self.output
            .instructions
            .push(Instruction::move_register(3, 27));
        self.record_relocation(RelocationKind::Rel24, &plan.predicate);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.predicate,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 3,
                clear: 24,
            });
        self.emit_branch_conditional_to(12, 2, false_result);
        self.output
            .instructions
            .push(Instruction::move_register(3, 26));
        self.output
            .instructions
            .push(Instruction::move_register(4, 27));
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 26,
            offset: i16::try_from(plan.vptr_offset)
                .map_err(|_| Diagnostic::error("virtual vptr offset is out of range"))?,
        });
        for (destination, home) in [(5, 28), (6, 29), (7, 30)] {
            self.output
                .instructions
                .push(Instruction::move_register(destination, home));
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 12,
            offset: i16::try_from(plan.slot_offset)
                .map_err(|_| Diagnostic::error("virtual slot offset is out of range"))?,
        });
        self.output
            .instructions
            .push(Instruction::move_register(8, 31));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.emit_branch_to(done);
        self.bind_label(false_result);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.bind_label(done);
        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: 26,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
