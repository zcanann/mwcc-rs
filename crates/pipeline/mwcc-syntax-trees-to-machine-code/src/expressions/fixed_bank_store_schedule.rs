//! Restore build 163's materialized fixed-bank constant-store order.
//!
//! The generic latency scheduler sees the constant value as independent and
//! moves it between `lis` and `addi`. Build 163 keeps the declared bank address
//! pair together, then materializes the value immediately before the store.

use super::*;

fn schedule_materialized_bank_store(instructions: &mut [Instruction]) -> bool {
    for start in 0..instructions.len().saturating_sub(3) {
        let Instruction::AddImmediateShifted { d: base, a: 0, .. } = instructions[start] else {
            continue;
        };
        if !matches!(
            instructions[start + 1],
            Instruction::AddImmediate { d: 0, a: 0, .. }
        ) || !matches!(
            instructions[start + 2],
            Instruction::AddImmediate { d, a, .. } if d == base && a == base
        ) {
            continue;
        }
        let is_store = match instructions[start + 3] {
            Instruction::StoreByte { s, a, .. }
            | Instruction::StoreHalfword { s, a, .. }
            | Instruction::StoreWord { s, a, .. } => s == 0 && a == base,
            _ => false,
        };
        if is_store {
            instructions.swap(start + 1, start + 2);
            return true;
        }
    }
    false
}

impl Generator {
    pub(crate) fn schedule_materialized_fixed_bank_store(&mut self) {
        if self.behavior.fixed_address_poll_address_style
            == mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            schedule_materialized_bank_store(&mut self.output.instructions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_complete_bank_address_before_the_constant() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(32, -13312),
            Instruction::load_immediate(0, 0),
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0x2000,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 32,
                offset: 2,
            },
        ];

        assert!(schedule_materialized_bank_store(&mut instructions));
        assert!(matches!(
            instructions[1],
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0x2000
            }
        ));
    }
}
