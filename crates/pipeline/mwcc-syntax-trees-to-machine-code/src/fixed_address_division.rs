//! Keep a directly loaded unsigned dividend in r0 and its multiplier separate.
//!
//! The source recognizer excludes named values, compound dividends, and narrow
//! loads. The ordinary constant-address cache owns the address lifetime; this
//! emitter owns only the multiply's two inputs and their local issue order.

use crate::generator::{Generator, GENERAL_SCRATCH};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Pointee, Type};
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_emit_fixed_address_unsigned_divide(
        &mut self,
        dividend: &Expression,
        magic: u32,
        shift: u8,
        destination: u8,
    ) -> Compilation<bool> {
        if self.behavior.optimization < Optimization::O2 {
            return Ok(false);
        }
        let Some(address) = fixed_word_dividend(dividend) else {
            return Ok(false);
        };
        let (address_high, offset) = crate::expressions::split_address(address);
        let (base, materialize) = if address_high == 0 {
            (0, false)
        } else {
            let Some(base) = self.claim_const_address_base_avoiding(address_high, Vec::new())
            else {
                return Ok(false);
            };
            base
        };
        let Some(multiplier) = (3..=12).find(|r| *r != base && !self.reserved.contains(r)) else {
            return Err(Diagnostic::error(
                "out of registers for fixed-address division",
            ));
        };
        let low = magic as i16;
        let high = (magic.wrapping_sub(low as i32 as u32) >> 16) as i16;
        if materialize {
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: base,
                    a: 0,
                    immediate: address_high,
                });
        }
        let load = Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: base,
            offset,
        };
        let constant_high = Instruction::AddImmediateShifted {
            d: multiplier,
            a: 0,
            immediate: high,
        };
        if self.behavior.scheduler_enabled {
            self.output.instructions.extend([constant_high, load]);
        } else {
            self.output.instructions.extend([load, constant_high]);
        }
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: multiplier,
                a: multiplier,
                immediate: low,
            },
            Instruction::MultiplyHighWordUnsigned {
                d: GENERAL_SCRATCH,
                a: multiplier,
                b: GENERAL_SCRATCH,
            },
            Instruction::ShiftRightLogicalImmediate {
                a: destination,
                s: GENERAL_SCRATCH,
                shift,
            },
        ]);
        Ok(true)
    }
}

/// Integer casts around a word load preserve its bits. Narrow, floating-point,
/// and wide conversions must still be evaluated by the ordinary operand path.
fn fixed_word_dividend(dividend: &Expression) -> Option<u32> {
    let mut value = dividend;
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = value
    {
        value = operand;
    }
    let Expression::Dereference { pointer } = value else {
        return None;
    };
    match crate::expressions::const_address_pointer(pointer)? {
        (Pointee::Int | Pointee::UnsignedInt, address) => Some(address),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cast(target_type: Type, operand: Expression) -> Expression {
        Expression::Cast {
            target_type,
            operand: Box::new(operand),
        }
    }

    fn load(pointee: Pointee, address: u32) -> Expression {
        Expression::Dereference {
            pointer: Box::new(cast(
                Type::Pointer(pointee),
                Expression::IntegerLiteral(address as i64),
            )),
        }
    }

    #[test]
    fn recognizes_word_loads_through_bit_preserving_casts() {
        for address in [0x100, 0x8000_00f8, 0x8000_8004, 0xffff_ffff] {
            for pointee in [Pointee::Int, Pointee::UnsignedInt] {
                let value = load(pointee, address);
                assert_eq!(fixed_word_dividend(&value), Some(address));
                let value = cast(Type::UnsignedInt, cast(Type::Int, value));
                assert_eq!(fixed_word_dividend(&value), Some(address));
            }
        }
    }

    #[test]
    fn retains_conversions_that_change_the_loaded_value() {
        for ty in [
            Type::Char,
            Type::UnsignedChar,
            Type::Short,
            Type::UnsignedShort,
            Type::Float,
            Type::Double,
            Type::LongLong,
            Type::UnsignedLongLong,
        ] {
            let value = cast(
                Type::UnsignedInt,
                cast(ty, load(Pointee::UnsignedInt, 0x8000_00f8)),
            );
            assert_eq!(fixed_word_dividend(&value), None);
        }
        for pointee in [
            Pointee::Char,
            Pointee::UnsignedChar,
            Pointee::Short,
            Pointee::UnsignedShort,
            Pointee::Float,
            Pointee::Double,
            Pointee::LongLong,
            Pointee::UnsignedLongLong,
        ] {
            assert_eq!(fixed_word_dividend(&load(pointee, 0x8000_00f8)), None);
        }
        assert_eq!(fixed_word_dividend(&Expression::IntegerLiteral(400)), None);
    }
}
