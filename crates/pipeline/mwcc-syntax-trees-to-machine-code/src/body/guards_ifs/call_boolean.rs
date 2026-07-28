//! Normalize one direct call's truth value through two constant returns.
//!
//! Legacy linkage-first builds keep forwarded scalar arguments in their incoming
//! ABI registers, call directly, then form the source `if (call)` / `if (!call)`
//! diamond around one shared LR-only epilogue.

#[allow(unused_imports)]
use super::*;

struct CallBoolean {
    callee: String,
    arguments: Vec<Expression>,
    zero_value: i16,
    nonzero_value: i16,
}

fn forwarded_parameter(expression: &Expression) -> Option<&str> {
    let mut expression = expression;
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn classify(function: &Function) -> Option<CallBoolean> {
    if matches!(
        function.return_type,
        Type::Void | Type::Float | Type::Double | Type::Struct { .. }
    ) || !function.locals.is_empty()
        || !function.statements.is_empty()
    {
        return None;
    }
    let [guard] = function.guards.as_slice() else {
        return None;
    };
    let fallback = i16::try_from(constant_value(function.return_expression.as_ref()?)?).ok()?;
    let guarded = i16::try_from(constant_value(&guard.value)?).ok()?;
    let (call, negated) = match &guard.condition {
        Expression::Call { .. } => (&guard.condition, false),
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } if matches!(operand.as_ref(), Expression::Call { .. }) => (operand.as_ref(), true),
        _ => return None,
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = call
    else {
        unreachable!("call shape was established above")
    };
    if arguments.len() > function.parameters.len()
        || arguments
            .iter()
            .zip(&function.parameters)
            .any(|(argument, parameter)| forwarded_parameter(argument) != Some(&parameter.name))
        || function.parameters[..arguments.len()]
            .iter()
            .any(|parameter| {
                matches!(
                    parameter.parameter_type,
                    Type::Float
                        | Type::Double
                        | Type::LongLong
                        | Type::UnsignedLongLong
                        | Type::Struct { .. }
                        | Type::Void
                )
            })
    {
        return None;
    }
    let (zero_value, nonzero_value) = if negated {
        (guarded, fallback)
    } else {
        (fallback, guarded)
    };
    Some(CallBoolean {
        callee: callee.clone(),
        arguments: arguments.clone(),
        zero_value,
        nonzero_value,
    })
}

impl Generator {
    pub(crate) fn try_call_boolean(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.locations.contains_key(&shape.callee)
            || self.globals.contains_key(&shape.callee)
        {
            return Ok(false);
        }

        let nonzero = self.fresh_label();
        let epilogue = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 8;
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
                offset: -8,
            },
        ]);
        self.emit_call(&shape.callee, &shape.arguments, None, false)?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, nonzero);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shape.zero_value));
        self.emit_branch_to(epilogue);
        self.bind_label(nonzero);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shape.nonzero_value));
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
