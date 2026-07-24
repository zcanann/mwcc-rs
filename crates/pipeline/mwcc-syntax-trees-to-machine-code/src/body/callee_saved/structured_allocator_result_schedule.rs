//! Result publication and initialization after a frame-cursor allocator call.
//!
//! The allocator returns an object while updating an address-taken cursor.
//! MWCC reloads the cursor first, publishes the result through the incoming
//! argument register, overlaps a read-only pool address, and retains one zero
//! register across the straight-line scalar initialization that follows.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_allocator_cursor_result(&mut self) {
        let Some(region) = allocator_cursor_result(&self.output) else {
            return;
        };
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[region.pool_high]
        else {
            unreachable!("the pool high half was matched")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        let Instruction::LoadFloatSingle { a, .. } = &mut self.output.instructions[region.pool_low]
        else {
            unreachable!("the pool load was matched")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.output.instructions[region.zero] = Instruction::load_immediate(5, 0);
        let Instruction::StoreHalfword { s, a, .. } =
            &mut self.output.instructions[region.first_store]
        else {
            unreachable!("the first member store was matched")
        };
        *s = 5;
        *a = Eabi::FIRST_GENERAL_ARGUMENT;

        self.move_instruction_before(region.cursor_reload, region.result_copy);
        self.move_instruction_before(region.zero, region.result_copy + 1);
        self.move_instruction_before(region.pool_high, region.result_copy + 2);
        self.reuse_allocator_initialization_zero(region.result_copy + 1, region.result, 5);
    }

    fn reuse_allocator_initialization_zero(&mut self, seed: usize, result: u8, zero: u8) {
        if !matches!(
            self.output.instructions.get(seed),
            Some(Instruction::AddImmediate {
                d,
                a: 0,
                immediate: 0,
            }) if *d == zero
        ) {
            return;
        }
        let mut at = seed + 1;
        while at + 1 < self.output.instructions.len() {
            if is_result_schedule_barrier(&self.output.instructions[at])
                || writes_result_schedule_register(&self.output.instructions[at], zero)
            {
                break;
            }
            if reusable_zero_store(&self.output.instructions[at..at + 2], result, &[8, 24, 26]) {
                let Instruction::StoreHalfword { s, .. } = &mut self.output.instructions[at + 1]
                else {
                    unreachable!("the reusable zero store was matched")
                };
                *s = zero;
                self.remove_allocator_result_instruction(at);
                at += 1;
            } else {
                at += 1;
            }
        }
    }

    fn remove_allocator_result_instruction(&mut self, index: usize) {
        let old_len = self.output.instructions.len();
        self.output.instructions.remove(index);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < index {
                    old
                } else if old == index {
                    index.saturating_sub(1)
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

#[derive(Clone, Copy)]
struct AllocatorCursorResult {
    result_copy: usize,
    cursor_reload: usize,
    zero: usize,
    first_store: usize,
    pool_high: usize,
    pool_low: usize,
    result: u8,
}

fn allocator_cursor_result(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<AllocatorCursorResult> {
    for result_copy in 1..output.instructions.len().saturating_sub(4) {
        if !matches!(
            output.instructions[result_copy - 1],
            Instruction::BranchAndLink { .. }
        ) {
            continue;
        }
        let [Instruction::Or {
            a: result,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        }, Instruction::LoadWord {
            d: cursor, a: 1, ..
        }, Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        }, Instruction::StoreHalfword {
            s: 0,
            a: store_base,
            offset: 0,
        }, ..] = &output.instructions[result_copy..]
        else {
            continue;
        };
        if result != store_base || result == cursor {
            continue;
        }
        let search_end = (result_copy + 12).min(output.instructions.len().saturating_sub(1));
        let Some(pool_high) = (result_copy + 4..search_end).find(|&high| {
            matches!(
                (&output.instructions[high], &output.instructions[high + 1]),
                (
                    Instruction::AddImmediateShifted {
                        d: base,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::LoadFloatSingle {
                        d: _,
                        a: load_base,
                        offset: 0,
                    },
                ) if base == load_base
            ) && output.relocations.iter().any(|relocation| {
                relocation.instruction_index == high && relocation.kind == RelocationKind::Addr16Ha
            }) && output.relocations.iter().any(|relocation| {
                relocation.instruction_index == high + 1
                    && relocation.kind == RelocationKind::Addr16Lo
            })
        }) else {
            continue;
        };
        if output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if (result_copy..=pool_high + 1).contains(target)
            )
        }) {
            continue;
        }
        return Some(AllocatorCursorResult {
            result_copy,
            cursor_reload: result_copy + 1,
            zero: result_copy + 2,
            first_store: result_copy + 3,
            pool_high,
            pool_low: pool_high + 1,
            result: *result,
        });
    }
    None
}

fn reusable_zero_store(window: &[Instruction], result: u8, offsets: &[i16]) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreHalfword {
                s,
                a,
                offset,
            },
            ..
        ] if zero == s && *a == result && offsets.contains(offset)
    )
}

fn writes_result_schedule_register(instruction: &Instruction, register: u8) -> bool {
    mwcc_vreg::register_operands(instruction)
        .iter()
        .any(|operand| {
            operand.role == mwcc_vreg::RegisterRole::Define
                && operand.class == mwcc_vreg::Class::General
                && operand.register == register
        })
}

fn is_result_schedule_barrier(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::BranchConditionalForward { .. }
            | Instruction::Branch { .. }
            | Instruction::BranchConditionalToLinkRegister { .. }
            | Instruction::BranchToLinkRegister
            | Instruction::BranchToLinkRegisterAndLink
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchToCountRegister
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::ReturnFromInterrupt
            | Instruction::SystemCall
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn allocator_result_output() -> mwcc_machine_code::MachineFunction {
        let mut output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::BranchAndLink {
                    target: "allocate".into(),
                },
                Instruction::move_register(31, 3),
                Instruction::LoadWord {
                    d: 29,
                    a: 1,
                    offset: 8,
                },
                Instruction::load_immediate(0, 0),
                Instruction::StoreHalfword {
                    s: 0,
                    a: 31,
                    offset: 0,
                },
                Instruction::LoadHalfwordZero {
                    d: 0,
                    a: 26,
                    offset: 8,
                },
                Instruction::ShiftLeftImmediate {
                    a: 3,
                    s: 0,
                    shift: 2,
                },
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 3,
                    offset: 0,
                },
            ],
            ..Default::default()
        };
        output.relocations = vec![
            Relocation {
                instruction_index: 7,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::Constant(0),
            },
            Relocation {
                instruction_index: 8,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::Constant(0),
            },
        ];
        output
    }

    #[test]
    fn recognizes_the_allocator_result_publication_window() {
        let output = allocator_result_output();
        let region = allocator_cursor_result(&output).expect("the result window should match");
        assert_eq!(
            (
                region.result_copy,
                region.cursor_reload,
                region.zero,
                region.first_store,
                region.pool_high,
                region.pool_low,
                region.result,
            ),
            (1, 2, 3, 4, 7, 8, 31)
        );
    }

    #[test]
    fn recognizes_only_the_family_member_zero_initializers() {
        assert!(reusable_zero_store(
            &[
                Instruction::load_immediate(0, 0),
                Instruction::StoreHalfword {
                    s: 0,
                    a: 31,
                    offset: 24,
                },
            ],
            31,
            &[8, 24, 26],
        ));
        assert!(!reusable_zero_store(
            &[
                Instruction::load_immediate(0, 0),
                Instruction::StoreHalfword {
                    s: 0,
                    a: 30,
                    offset: 24,
                },
            ],
            31,
            &[8, 24, 26],
        ));
    }
}
