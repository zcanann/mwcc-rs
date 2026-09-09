//! The patched 2.3.3 scheduler completes a retained address after the quotient.
//!
//! Keep the address high half distinct from its final value, then nominate the
//! measured consumer order. Ordinary liveness decides which homes can overlap.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, Relocation, RelocationKind, RelocationTarget};
use mwcc_versions::{DivisionAddressSchedule, Optimization};
use mwcc_vreg::{register_operands, Class, Reg};

#[derive(Debug, PartialEq, Eq)]
struct Plan {
    low: usize,
    quotient: usize,
    address: u32,
    sign: u32,
    result: u32,
    dividend: u32,
}

impl Generator {
    pub(crate) fn schedule_division_address_lows(&mut self) {
        if self.behavior.division_address_schedule != DivisionAddressSchedule::LowAfterQuotient
            || !self.behavior.scheduler_enabled
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
            || !self.output.jump_tables.is_empty()
            || self.output.instructions.iter().any(|i| {
                matches!(
                    i,
                    Instruction::VerbatimWord { .. } | Instruction::BranchToCountRegister
                )
            })
        {
            return;
        }
        for start in 0..self.output.instructions.len() {
            let Some(plan) = plan(&self.output.instructions, &self.output.relocations, start)
            else {
                continue;
            };
            // A branch-local copy of the quotient should retain its selected
            // home when the two lifetimes permit coalescing.
            if matches!(
                self.output.instructions.get(plan.quotient + 1),
                Some(Instruction::BranchConditionalForward { .. })
            ) {
                let copied = match self.output.instructions.get(plan.quotient + 2) {
                    Some(Instruction::Or { a, s, b }) if *s == plan.result && s == b => Some(*a),
                    Some(Instruction::AddImmediate { d, a, immediate: 0 }) if *a == plan.result => {
                        Some(*d)
                    }
                    _ => None,
                };
                if let Some(destination) =
                    copied.and_then(|r| Reg::from_field(r, Class::General).virtual_register())
                {
                    self.register_affinity.insert(
                        destination,
                        Reg::from_field(plan.result, Class::General)
                            .virtual_register()
                            .unwrap(),
                    );
                }
            }
            let high = self.fresh_virtual_general();
            if let Instruction::AddImmediateShifted { d, .. } =
                &mut self.output.instructions[plan.low - 1]
            {
                *d = high;
            }
            if let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[plan.low] {
                *a = high;
            }
            // This schedule owns the joint division/address group. Its first
            // ownership takes precedence over the ordinary division group.
            self.consumer_allocation_groups.insert(
                0,
                [plan.address, plan.sign, high, plan.result, plan.dividend]
                    .into_iter()
                    .map(|r| {
                        Reg::from_field(r, Class::General)
                            .virtual_register()
                            .unwrap()
                    })
                    .collect(),
            );
            let length = plan.quotient - plan.low + 1;
            let schedule: Vec<_> = (1..length).chain(std::iter::once(0)).collect();
            crate::permute_machine_function_region(&mut self.output, plan.low, &schedule);
        }
    }
}

fn plan(instructions: &[Instruction], relocations: &[Relocation], start: usize) -> Option<Plan> {
    let [Instruction::AddImmediateShifted {
        d: constant_high,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: 0,
        a: constant_input,
        ..
    }, Instruction::AddImmediateShifted {
        d: address, a: 0, ..
    }, Instruction::AddImmediate {
        d: address_out,
        a: address_input,
        ..
    }, Instruction::LoadWord { d: dividend, .. }, Instruction::MultiplyHighWord {
        d: 0,
        a: 0,
        b: multiplied,
    }] = instructions.get(start..start + 6)?
    else {
        return None;
    };
    if constant_high != constant_input
        || address != address_out
        || address != address_input
        || dividend != multiplied
    {
        return None;
    }
    let target = |index, kind| {
        relocations
            .iter()
            .find(|r| r.instruction_index == index && r.kind == kind)
            .and_then(|r| match &r.target {
                RelocationTarget::External(name) => Some(name.as_str()),
                _ => None,
            })
    };
    let high_target = target(start + 2, RelocationKind::Addr16Ha)?;
    if target(start + 3, RelocationKind::Addr16Lo) != Some(high_target)
        || target(start + 4, RelocationKind::EmbSda21).is_none()
    {
        return None;
    }
    let mut next = start + 6;
    if matches!(instructions.get(next), Some(Instruction::Add { d: 0, a: 0, b }) if b == dividend) {
        next += 1;
    }
    if matches!(
        instructions.get(next),
        Some(Instruction::ShiftRightAlgebraicImmediate { a: 0, s: 0, .. })
    ) {
        next += 1;
    }
    let Instruction::ShiftRightLogicalImmediate {
        a: sign,
        s: 0,
        shift: 31,
    } = instructions.get(next)?
    else {
        return None;
    };
    let (Instruction::Add { d: result, a: 0, b } | Instruction::AddRecord { d: result, a: 0, b }) =
        instructions.get(next + 1)?
    else {
        return None;
    };
    if b != sign
        || ![address, dividend, sign, result]
            .into_iter()
            .all(|r| Reg::is_virtual_field(*r))
    {
        return None;
    }
    let quotient = next + 1;
    if instructions[start + 4..=quotient]
        .iter()
        .flat_map(register_operands)
        .any(|operand| operand.class == Class::General && operand.register == *address)
        || instructions.iter().any(|instruction| {
            matches!(instruction,
            Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. }
            if (start + 1..=quotient).contains(target))
        })
        || relocations.iter().any(|r| {
            (start..=quotient).contains(&r.instruction_index)
                && ![start + 2, start + 3, start + 4].contains(&r.instruction_index)
        })
    {
        return None;
    }
    Some(Plan {
        low: start + 3,
        quotient,
        address: *address,
        sign: *sign,
        result: *result,
        dividend: *dividend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::RelocationTarget;

    fn sequence() -> Vec<Instruction> {
        vec![
            Instruction::AddImmediateShifted {
                d: 34,
                a: 0,
                immediate: 0x6666,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 34,
                immediate: 0x6667,
            },
            Instruction::AddImmediateShifted {
                d: 32,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 33,
                a: 0,
                offset: 0,
            },
            Instruction::MultiplyHighWord { d: 0, a: 0, b: 33 },
            Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 0,
                shift: 6,
            },
            Instruction::ShiftRightLogicalImmediate {
                a: 35,
                s: 0,
                shift: 31,
            },
            Instruction::AddRecord { d: 36, a: 0, b: 35 },
        ]
    }

    fn relocations() -> Vec<Relocation> {
        [
            (2, RelocationKind::Addr16Ha, "packet"),
            (3, RelocationKind::Addr16Lo, "packet"),
            (4, RelocationKind::EmbSda21, "input"),
        ]
        .into_iter()
        .map(|(instruction_index, kind, name)| Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(name.into()),
        })
        .collect()
    }

    #[test]
    fn an_address_low_can_follow_signed_quotient_rounding() {
        assert_eq!(
            plan(&sequence(), &relocations(), 0),
            Some(Plan {
                low: 3,
                quotient: 8,
                address: 32,
                sign: 35,
                result: 36,
                dividend: 33
            })
        );
        let mut instructions = sequence();
        instructions.insert(6, Instruction::Add { d: 0, a: 0, b: 33 });
        assert_eq!(plan(&instructions, &relocations(), 0).unwrap().quotient, 9);
    }

    #[test]
    fn branch_entries_and_intermediate_address_uses_prevent_deferral() {
        for target in 1..=8 {
            let mut instructions = sequence();
            instructions.push(Instruction::Branch { target });
            assert_eq!(plan(&instructions, &relocations(), 0), None);
        }
        let mut instructions = sequence();
        instructions[4] = Instruction::LoadWord {
            d: 33,
            a: 32,
            offset: 0,
        };
        assert_eq!(plan(&instructions, &relocations(), 0), None);
    }

    #[test]
    fn address_halves_must_share_a_symbol_and_rounding_must_be_unrelocated() {
        let mut fixups = relocations();
        fixups[1].target = RelocationTarget::External("other".into());
        assert_eq!(plan(&sequence(), &fixups, 0), None);
        fixups = relocations();
        fixups.push(Relocation {
            instruction_index: 6,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External("other".into()),
        });
        assert_eq!(plan(&sequence(), &fixups, 0), None);
    }
}
