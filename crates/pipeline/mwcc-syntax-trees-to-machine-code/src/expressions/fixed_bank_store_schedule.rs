//! Restore build 163's materialized fixed-bank constant-store order.
//!
//! The generic latency scheduler sees the constant value as independent and
//! moves it between `lis` and `addi`. Build 163 keeps the declared bank address
//! pair together, then materializes the value immediately before the store.

use super::*;

fn materialized_bank_store(
    instructions: &[Instruction],
    symbolic: &std::collections::HashSet<usize>,
) -> Option<usize> {
    for start in 0..instructions.len().saturating_sub(3) {
        // A symbol-address pair is not a fixed hardware bank. Its zero
        // immediates are relocation placeholders, not literal address bits.
        if (start..start + 3).any(|at| symbolic.contains(&at)) {
            continue;
        }
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
            return Some(start);
        }
    }
    None
}

impl Generator {
    pub(crate) fn schedule_materialized_fixed_bank_store(&mut self) {
        if self.behavior.fixed_address_poll_address_style
            == mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            let symbolic = self
                .output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .chain(
                    self.output
                        .deferred_displacements
                        .iter()
                        .map(|displacement| displacement.instruction_index),
                )
                .collect();
            if let Some(start) = materialized_bank_store(&self.output.instructions, &symbolic) {
                crate::move_instruction_before_retargeting(self, start + 2, start + 1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_complete_bank_address_before_the_constant() {
        let instructions = vec![
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

        assert_eq!(
            materialized_bank_store(&instructions, &Default::default()),
            Some(0)
        );
        // The same machine shape can instead be an unresolved global address.
        // Neither relocations nor deferred data-section displacements may be
        // mistaken for a literal bank schedule.
        for symbolic in [vec![0, 2], vec![2]] {
            assert_eq!(
                materialized_bank_store(&instructions, &symbolic.into_iter().collect()),
                None
            );
        }
    }
}
