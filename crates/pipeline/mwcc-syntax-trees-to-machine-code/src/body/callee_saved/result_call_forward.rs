//! General-result forwarding into an immediately following mixed-register call.
//!
//! These shapes belong outside the callee-saved families: the forwarded value
//! dies in the consumer and therefore never needs a callee-saved home. This module
//! owns the schedule from the value's producer through the consumer call.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `int x = make(...); trace(K, "...", x); return C;` on the predecrement
    /// generations. The producer consumes every live-in before its result is
    /// forwarded, so `x` never needs a callee-saved home: it remains in r3 until
    /// the consumer setup moves it directly to r5. MWCC splits the string address
    /// around that move and the leading integer argument, filling the `lis`
    /// latency window without extending the result's lifetime.
    pub(crate) fn try_result_trace_forward_constant_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let Some(return_constant) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .filter(|value| i16::try_from(*value).is_ok())
        else {
            return Ok(false);
        };
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.is_static
            || local.array_length.is_some()
            || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let Some((producer_name, producer_arguments)) =
            direct_call_through_value_cast(local.initializer.as_ref())
        else {
            return Ok(false);
        };
        let [Statement::Expression(Expression::Call {
            name: consumer_name,
            arguments: consumer_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Expression::IntegerLiteral(leading), Expression::StringLiteral(string), Expression::Variable(forwarded)] =
            consumer_arguments.as_slice()
        else {
            return Ok(false);
        };
        if forwarded != &local.name || i16::try_from(*leading).is_err() {
            return Ok(false);
        }
        // A short literal may use SDA21 rather than the measured split absolute
        // address. This owner only claims strings whose address is known to be a
        // `lis`/`addi` pair under the active data model.
        if self.behavior.global_addressing != GlobalAddressing::Absolute && string.len() + 1 <= 8 {
            return Ok(false);
        }
        for name in [producer_name, consumer_name.as_str()] {
            if self.locations.contains_key(name) || self.globals.contains_key(name) {
                return Ok(false);
            }
        }
        let packed = self.behavior.forwarded_trace_string_style
            == mwcc_versions::ForwardedTraceStringStyle::PackedLowBeforeInteger;
        if self.behavior.string_literals_packed && !packed {
            return Ok(false);
        }

        self.non_leaf = true;
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
        self.emit_call(producer_name, producer_arguments, None, false)?;

        self.output.packed_string_literals = packed;
        let placeholder = self.string_literal_placeholder(string);
        self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT + 1, &placeholder);
        self.output.instructions.push(Instruction::move_register(
            Eabi::FIRST_GENERAL_ARGUMENT + 2,
            Eabi::general_result().number,
        ));
        if packed {
            self.emit_string_address_low(
                &placeholder,
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
            );
            self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT, *leading);
        } else {
            self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT, *leading);
            self.emit_string_address_low(
                &placeholder,
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
            );
        }
        self.emit_forward_consumer_link(consumer_name);
        self.emit_non_leaf_constant_join_epilogue(return_constant);
        Ok(true)
    }

    /// `x = parameter->member; use(x, integer, float);` for the legacy
    /// linkage-first frame convention.
    ///
    /// The local is a zero-cost name for a word member load into r3. Build 163
    /// hoists the integer literal between `mflr` and the LR store, creates its
    /// eight-byte linkage frame, then loads the member and the independent float
    /// literal before calling the consumer.
    pub(crate) fn try_computed_local_call_forward(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if class_of(parameter.parameter_type)? != ValueClass::General
            || self
                .locations
                .get(parameter.name.as_str())
                .map(|location| location.register)
                != Some(Eabi::FIRST_GENERAL_ARGUMENT)
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.is_static
            || local.array_length.is_some()
            || class_of(local.declared_type)? != ValueClass::General
            || local.declared_type.width() != 32
        {
            return Ok(false);
        }
        let Some(Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        }) = value_cast_operand(local.initializer.as_ref())
        else {
            return Ok(false);
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == &parameter.name)
            || class_of(*member_type)? != ValueClass::General
            || member_type.width() != 32
            || i16::try_from(*offset).is_err()
        {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call {
            name: consumer_name,
            arguments: consumer_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some((integer, float)) =
            mixed_forward_literals(consumer_arguments, local.name.as_str())
        else {
            return Ok(false);
        };
        if !self.mixed_forward_parameter_types_match(consumer_name)
            || self.locations.contains_key(consumer_name)
            || self.globals.contains_key(consumer_name)
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 8;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.evaluate_general(integer, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
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
                offset: -8,
            });
        self.evaluate_general(
            local
                .initializer
                .as_ref()
                .expect("member initializer checked"),
            Eabi::FIRST_GENERAL_ARGUMENT,
        )?;
        self.evaluate_float(float, Eabi::FIRST_FLOAT_ARGUMENT)?;
        self.emit_forward_consumer_link(consumer_name);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `x = make(parameters...); use(x, integer, float);` in a void function.
    ///
    /// Every incoming parameter feeds the producer in its natural ABI lane and
    /// dies there. The produced value remains in r3 for the consumer. In the
    /// measured predecrement schedule, the mixed tail fills f1 before r4 even
    /// though the integer precedes the float in source order.
    pub(crate) fn try_result_call_forward_with_live_ins(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention == FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.parameters.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.is_static
            || local.array_length.is_some()
            || class_of(local.declared_type)? != ValueClass::General
            || local.declared_type.width() != 32
        {
            return Ok(false);
        }
        let Some((producer_name, producer_arguments)) =
            direct_call_through_value_cast(local.initializer.as_ref())
        else {
            return Ok(false);
        };
        let [Statement::Expression(Expression::Call {
            name: consumer_name,
            arguments: consumer_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some((integer, float)) =
            mixed_forward_literals(consumer_arguments, local.name.as_str())
        else {
            return Ok(false);
        };
        if producer_arguments.len() != function.parameters.len()
            || !producer_arguments
                .iter()
                .zip(&function.parameters)
                .all(|(argument, parameter)| {
                    matches!(argument, Expression::Variable(name) if name == &parameter.name)
                })
        {
            return Ok(false);
        }
        // Prove the producer consumes each incoming parameter in its assigned
        // EABI lane, so setting up the call emits no moves and no live-in survives.
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let expected = match class_of(parameter.parameter_type)? {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            if self
                .locations
                .get(parameter.name.as_str())
                .map(|location| location.register)
                != Some(expected)
            {
                return Ok(false);
            }
        }
        if !self.mixed_forward_parameter_types_match(consumer_name) {
            return Ok(false);
        }
        for name in [producer_name, consumer_name] {
            if self.locations.contains_key(name) || self.globals.contains_key(name) {
                return Ok(false);
            }
        }

        self.non_leaf = true;
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
        self.emit_call(producer_name, producer_arguments, None, false)?;
        self.evaluate_float(float, Eabi::FIRST_FLOAT_ARGUMENT)?;
        self.evaluate_general(integer, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
        self.emit_forward_consumer_link(consumer_name);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn mixed_forward_parameter_types_match(&self, consumer_name: &str) -> bool {
        self.call_parameter_types
            .get(consumer_name)
            .map(|types| {
                matches!(types.as_slice(), [first, second, third]
                    if !matches!(first, Type::Float | Type::Double)
                        && !matches!(second, Type::Float | Type::Double)
                        && matches!(third, Type::Float))
            })
            .unwrap_or(true)
    }

    fn emit_forward_consumer_link(&mut self, consumer_name: &str) {
        if self.variadic_callees.contains(consumer_name) {
            self.output
                .instructions
                .push(Instruction::ConditionRegisterClear { d: 6 });
        }
        self.record_relocation(RelocationKind::Rel24, consumer_name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: consumer_name.to_string(),
        });
    }
}

fn mixed_forward_literals<'a>(
    arguments: &'a [Expression],
    local_name: &str,
) -> Option<(&'a Expression, &'a Expression)> {
    let [Expression::Variable(forwarded), integer @ Expression::IntegerLiteral(value), float @ Expression::FloatLiteral(_)] =
        arguments
    else {
        return None;
    };
    (forwarded == local_name && *value >= i16::MIN as i64 && *value <= i16::MAX as i64)
        .then_some((integer, float))
}

fn value_cast_operand(initializer: Option<&Expression>) -> Option<&Expression> {
    match initializer? {
        Expression::Cast { operand, .. } => Some(operand),
        expression => Some(expression),
    }
}

fn direct_call_through_value_cast(
    initializer: Option<&Expression>,
) -> Option<(&str, &[Expression])> {
    match initializer? {
        Expression::Call { name, arguments } => Some((name, arguments)),
        Expression::Cast { operand, .. } => match operand.as_ref() {
            Expression::Call { name, arguments } => Some((name, arguments)),
            _ => None,
        },
        _ => None,
    }
}
