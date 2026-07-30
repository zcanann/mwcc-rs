//! Split address materialization for callback-tail call arguments.
//!
//! Legacy MWCC borrows an earlier, not-yet-finalized ABI argument register for
//! a callback address's high half. The low half publishes the address into its
//! final argument register before the borrowed register is overwritten.

use super::*;

enum CallbackTail<'a> {
    GlobalMember {
        first: &'a Expression,
        middle: i16,
        callback: &'a str,
    },
    LargeObject {
        addressed: &'a str,
        middle: i16,
        third: i16,
        callback: &'a str,
    },
}

fn callback_tail(arguments: &[Expression]) -> Option<CallbackTail<'_>> {
    match arguments {
        [first @ Expression::Member { .. }, Expression::IntegerLiteral(middle), Expression::Variable(callback)] => {
            Some(CallbackTail::GlobalMember {
                first,
                middle: i16::try_from(*middle).ok()?,
                callback,
            })
        }
        [Expression::AddressOf { operand }, Expression::IntegerLiteral(middle), Expression::IntegerLiteral(third), Expression::Variable(callback)] =>
        {
            let Expression::Variable(addressed) = operand.as_ref() else {
                return None;
            };
            Some(CallbackTail::LargeObject {
                addressed,
                middle: i16::try_from(*middle).ok()?,
                third: i16::try_from(*third).ok()?,
                callback,
            })
        }
        _ => None,
    }
}

impl Generator {
    /// Marshal a terminal callback address through a borrowed earlier argument
    /// register. The two supported prefixes share one rule: the borrowed
    /// register has no live final argument until after the callback low half.
    pub(crate) fn try_emit_split_callback_tail_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
        direct_call: bool,
    ) -> Compilation<bool> {
        let Some(shape) = callback_tail(arguments) else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !self.call_parameter_types.get(name).is_none_or(|types| {
                types.len() >= arguments.len()
                    && types[..arguments.len()]
                        .iter()
                        .all(|ty| !matches!(ty, Type::Float | Type::Double))
            })
        {
            return Ok(false);
        }

        match shape {
            CallbackTail::GlobalMember {
                first,
                middle,
                callback,
            } => {
                let Expression::Member {
                    base,
                    offset,
                    member_type,
                    index_stride: None,
                } = first
                else {
                    unreachable!()
                };
                let Expression::Variable(base) = base.as_ref() else {
                    return Ok(false);
                };
                let Some(pointee) = pointee_of_type(*member_type) else {
                    return Ok(false);
                };
                let Ok(offset) = i16::try_from(*offset) else {
                    return Ok(false);
                };
                if !matches!(
                    self.globals.get(base.as_str()),
                    Some(Type::StructPointer { .. })
                ) || matches!(pointee, Pointee::Float | Pointee::Double)
                    || !self.is_direct_function_symbol(callback)
                {
                    return Ok(false);
                }

                let borrowed = Eabi::FIRST_GENERAL_ARGUMENT;
                self.emit_split_callback_address(
                    callback,
                    borrowed,
                    Eabi::FIRST_GENERAL_ARGUMENT + 2,
                );
                // The callback high half leaves r4 live until the low half
                // publishes r5. Reuse that now-dead register for the global
                // pointer rather than overwriting the value argument in r3.
                self.emit_global_load_value(base, Eabi::FIRST_GENERAL_ARGUMENT + 1)?;
                self.output.instructions.push(displacement_load(
                    pointee,
                    Eabi::FIRST_GENERAL_ARGUMENT,
                    Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    offset,
                )?);
                self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT + 1, i64::from(middle));
            }
            CallbackTail::LargeObject {
                addressed,
                middle,
                third,
                callback,
            } => {
                let large = self.behavior.global_addressing == GlobalAddressing::Absolute
                    || self
                        .global_array_sizes
                        .get(addressed)
                        .is_some_and(|size| *size > 8)
                    || self
                        .addressable_globals
                        .get(addressed)
                        .is_some_and(|ty| match ty {
                            Type::Struct { size, .. } => *size > 8,
                            other => other.width() > 64,
                        });
                if !large || !self.is_direct_function_symbol(callback) {
                    return Ok(false);
                }

                self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT, addressed);
                self.emit_split_callback_address(
                    callback,
                    Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    Eabi::FIRST_GENERAL_ARGUMENT + 3,
                );
                self.record_relocation(RelocationKind::Addr16Lo, addressed);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: Eabi::FIRST_GENERAL_ARGUMENT,
                    a: Eabi::FIRST_GENERAL_ARGUMENT,
                    immediate: 0,
                });
                self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT + 1, i64::from(middle));
                self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT + 2, i64::from(third));
            }
        }
        Ok(true)
    }

    fn is_direct_function_symbol(&self, name: &str) -> bool {
        self.call_return_types.contains_key(name)
            && !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
    }

    fn emit_split_callback_address(&mut self, name: &str, high: u8, destination: u8) {
        self.emit_address_high(high, name);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: high,
            immediate: 0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_member_and_large_object_callback_tails() {
        let member = Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset: 8,
            member_type: Type::UnsignedChar,
            index_stride: None,
        };
        assert!(matches!(
            callback_tail(&[
                member,
                Expression::IntegerLiteral(10),
                Expression::Variable("done".into()),
            ]),
            Some(CallbackTail::GlobalMember { middle: 10, .. })
        ));
        assert!(matches!(
            callback_tail(&[
                Expression::AddressOf {
                    operand: Box::new(Expression::Variable("buffer".into())),
                },
                Expression::IntegerLiteral(32),
                Expression::IntegerLiteral(1056),
                Expression::Variable("done".into()),
            ]),
            Some(CallbackTail::LargeObject {
                middle: 32,
                third: 1056,
                ..
            })
        ));
    }
}
