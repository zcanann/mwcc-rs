//! Incoming stack values have allocator-owned identities and caller-SP offsets.
//! Loads enter the selected entry block before their first use or control edge.
//! Their displacements stay symbolic through frame resizing and scheduling.

use crate::generator::Generator;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{DeferredDisplacement, DeferredDisplacementTarget, Instruction};
use mwcc_vreg::{Class, RegisterRole};

#[derive(Clone, Debug)]
pub(crate) struct StackParameter {
    pub register: u8,
    pub offset: i16,
    pub width: u8,
    pub signed: bool,
}

impl Generator {
    pub(crate) fn materialize_incoming_stack_parameters(&mut self) -> Compilation<()> {
        for parameter in self.incoming_stack_parameters.clone() {
            let Some(at) = entry_load_position(&self.output.instructions, parameter.register)
            else {
                continue;
            };
            // Keep the provisional displacement outside local/save slots so
            // legacy frame relayout cannot mistake this load for a GPR restore.
            let provisional = self
                .frame_size
                .checked_add(parameter.offset)
                .ok_or_else(|| {
                    Diagnostic::error("incoming parameter displacement is out of range")
                })?;
            let load = match (parameter.width, parameter.signed) {
                (8, _) => Instruction::LoadByteZero {
                    d: parameter.register,
                    a: 1,
                    offset: provisional,
                },
                (16, true) => Instruction::LoadHalfwordAlgebraic {
                    d: parameter.register,
                    a: 1,
                    offset: provisional,
                },
                (16, false) => Instruction::LoadHalfwordZero {
                    d: parameter.register,
                    a: 1,
                    offset: provisional,
                },
                _ => Instruction::LoadWord {
                    d: parameter.register,
                    a: 1,
                    offset: provisional,
                },
            };
            crate::insert_instruction_retargeting(self, at, load);
            if parameter.width == 8 && parameter.signed {
                crate::insert_instruction_retargeting(
                    self,
                    at + 1,
                    Instruction::ExtendSignByte {
                        a: parameter.register,
                        s: parameter.register,
                    },
                );
            }
            self.output
                .deferred_displacements
                .push(DeferredDisplacement {
                    instruction_index: at,
                    target: DeferredDisplacementTarget::IncomingStack(parameter.offset),
                });
        }
        Ok(())
    }

    pub(crate) fn resolve_incoming_stack_displacements(&mut self) -> Compilation<()> {
        for fixup in &self.output.deferred_displacements {
            let DeferredDisplacementTarget::IncomingStack(offset) = fixup.target else {
                continue;
            };
            let displacement =
                resolve_stack_offset(&self.output.instructions[..fixup.instruction_index], offset)?;
            let offset = match &mut self.output.instructions[fixup.instruction_index] {
                Instruction::LoadWord { a: 1, offset, .. }
                | Instruction::LoadByteZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordAlgebraic { a: 1, offset, .. } => offset,
                _ => return Err(Diagnostic::error("incoming parameter fixup lost its load")),
            };
            *offset = displacement;
        }
        self.output
            .deferred_displacements
            .retain(|fixup| !matches!(fixup.target, DeferredDisplacementTarget::IncomingStack(_)));
        Ok(())
    }
}

fn entry_load_position(instructions: &[Instruction], register: u8) -> Option<usize> {
    let first_use = instructions.iter().position(|instruction| {
        mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| {
                operand.class == Class::General
                    && operand.role == RegisterRole::Use
                    && operand.register == register
            })
    });
    let first_use = first_use?;
    let boundary = instructions
        .iter()
        .position(|instruction| match instruction {
            Instruction::BranchAndLink { target } => {
                !target.starts_with("_savegpr_") && !target.starts_with("_savefpr_")
            }
            Instruction::Branch { .. }
            | Instruction::BranchConditionalForward { .. }
            | Instruction::BranchToLinkRegister
            | Instruction::BranchToCountRegister
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::BranchToLinkRegisterAndLink
            | Instruction::BranchConditionalToLinkRegister { .. } => true,
            _ => false,
        })
        .unwrap_or(first_use);
    let at = first_use.min(boundary);
    let overwritten = instructions[..at].iter().any(|instruction| {
        mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| {
                operand.class == Class::General
                    && operand.role == RegisterRole::Define
                    && operand.register == register
            })
    });
    (!overwritten).then_some(at)
}

fn resolve_stack_offset(prefix: &[Instruction], caller_offset: i16) -> Compilation<i16> {
    let mut adjustment = 0i32;
    for instruction in prefix {
        match instruction {
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset }
            | Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: offset,
            } => {
                adjustment += i32::from(*offset);
            }
            _ if mwcc_vreg::register_operands(instruction)
                .iter()
                .any(|operand| {
                    operand.class == Class::General
                        && operand.role == RegisterRole::Define
                        && operand.register == 1
                }) =>
            {
                return Err(Diagnostic::error(
                    "incoming parameter has an untracked stack base",
                ))
            }
            _ => {}
        }
    }
    i16::try_from(i32::from(caller_offset) - adjustment)
        .map_err(|_| Diagnostic::error("incoming parameter displacement is out of range"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_load_dominates_calls_and_conditional_assignments() {
        let call = Instruction::BranchAndLink {
            target: "mutate".into(),
        };
        let use_value = Instruction::move_register(3, 32);
        assert_eq!(entry_load_position(&[call, use_value.clone()], 32), Some(0));
        let branch = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 2,
        };
        assert_eq!(
            entry_load_position(
                &[
                    branch,
                    Instruction::load_immediate(32, 5),
                    use_value.clone(),
                ],
                32
            ),
            Some(0)
        );
        assert_eq!(
            entry_load_position(&[Instruction::load_immediate(32, 5), use_value,], 32),
            None
        );
    }

    #[test]
    fn entry_load_follows_dense_save_helpers() {
        assert_eq!(
            entry_load_position(
                &[
                    Instruction::BranchAndLink {
                        target: "_savegpr_25".into()
                    },
                    Instruction::move_register(31, 32),
                ],
                32
            ),
            Some(1)
        );
    }

    #[test]
    fn resolves_against_the_stack_position_at_the_load() {
        let push = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -64,
        };
        let pop = Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        };
        assert_eq!(resolve_stack_offset(&[], 11).unwrap(), 11);
        assert_eq!(resolve_stack_offset(&[push.clone()], 11).unwrap(), 75);
        assert_eq!(resolve_stack_offset(&[push, pop], 11).unwrap(), 11);
        assert!(resolve_stack_offset(&[Instruction::move_register(1, 3)], 8).is_err());
    }
}
