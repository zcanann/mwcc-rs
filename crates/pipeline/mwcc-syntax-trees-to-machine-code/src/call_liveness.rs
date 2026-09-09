//! ABI inputs recovered from declarations after instruction scheduling.
//!
//! A call can consume an incoming argument or a preceding call's result without
//! a materialization instruction. Those values still need a live range through
//! the consuming branch. Resolving named calls here avoids stale instruction
//! indices after scheduling; indirect and unprototyped calls retain the existing
//! materialized-input inference until call-site type facts are available.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Type;
use mwcc_target::Eabi;
use mwcc_vreg::Class;
use std::collections::HashMap;

impl Generator {
    pub(crate) fn declared_call_inputs(&self) -> HashMap<usize, Vec<(Class, u8)>> {
        self.output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                let Instruction::BranchAndLink { target } = instruction else {
                    return None;
                };
                let parameters = self.call_parameter_types.get(target)?;
                let hidden_result = matches!(
                    self.call_return_types.get(target),
                    Some(Type::Struct { .. })
                );
                Some((index, parameter_registers(parameters, hidden_result)))
            })
            .collect()
    }
}

/// EABI uses independent GPR/FPR cursors. Wide integers start at an odd GPR;
/// aggregates consume the address of the caller's copy. A memory result adds
/// its hidden pointer before the source parameters. Stack arguments introduce
/// no additional register use at the branch.
fn parameter_registers(parameters: &[Type], hidden_result: bool) -> Vec<(Class, u8)> {
    let mut general = usize::from(Eabi::FIRST_GENERAL_ARGUMENT);
    let mut floating = usize::from(Eabi::FIRST_FLOAT_ARGUMENT);
    let mut inputs = Vec::new();
    if hidden_result {
        inputs.push((Class::General, general as u8));
        general += 1;
    }
    for parameter in parameters {
        if matches!(parameter, Type::Float | Type::Double) {
            if floating <= usize::from(Eabi::LAST_FLOAT_ARGUMENT) {
                inputs.push((Class::Float, floating as u8));
            }
            floating += 1;
        } else {
            let wide = matches!(parameter, Type::LongLong | Type::UnsignedLongLong);
            if wide && general % 2 == 0 {
                general += 1;
            }
            for _ in 0..if wide { 2 } else { 1 } {
                if general <= usize::from(Eabi::LAST_GENERAL_ARGUMENT) {
                    inputs.push((Class::General, general as u8));
                }
                general += 1;
            }
        }
    }
    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligns_wide_inputs_without_consuming_gprs_for_floats() {
        assert_eq!(
            parameter_registers(
                &[
                    Type::Int,
                    Type::Double,
                    Type::LongLong,
                    Type::Float,
                    Type::Int
                ],
                false
            ),
            [
                (Class::General, 3),
                (Class::Float, 1),
                (Class::General, 5),
                (Class::General, 6),
                (Class::Float, 2),
                (Class::General, 7)
            ]
        );
    }

    #[test]
    fn includes_hidden_results_and_stops_at_register_overflow() {
        assert_eq!(
            parameter_registers(&[Type::Int; 10], true),
            (3..=10).map(|r| (Class::General, r)).collect::<Vec<_>>()
        );
        assert_eq!(
            parameter_registers(&[Type::Double; 10], false),
            (1..=8).map(|r| (Class::Float, r)).collect::<Vec<_>>()
        );
        assert!(parameter_registers(&[], false).is_empty());
    }
}
