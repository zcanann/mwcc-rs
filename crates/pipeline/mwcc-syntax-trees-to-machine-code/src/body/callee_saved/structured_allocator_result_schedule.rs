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
        self.schedule_allocator_initialization_prefix(region.result);
        self.fold_allocator_direct_call_result_stores();
        self.schedule_structured_guarded_ucode_packets();
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

    fn schedule_allocator_initialization_prefix(&mut self, result: u8) {
        let Some(prefix) = allocator_initialization_prefix(&self.output, result) else {
            return;
        };

        self.move_instruction_before(prefix.constant_high, prefix.first_width);
        let prefix = allocator_initialization_prefix(&self.output, result)
            .expect("moving the independent high half preserves the prefix");
        self.move_instruction_before(prefix.pool_low, prefix.first_width);
        let prefix = allocator_initialization_prefix(&self.output, result)
            .expect("moving the pool low half preserves the prefix");
        self.move_instruction_before(prefix.constant_low, prefix.first_width);
        let prefix = allocator_initialization_prefix(&self.output, result)
            .expect("moving the independent low half preserves the prefix");
        assign_allocator_initialization_registers(
            &mut self.output.instructions,
            prefix.object,
            result,
        );
        self.move_instruction_before(prefix.compare, prefix.first_shift);
    }

    fn fold_allocator_direct_call_result_stores(&mut self) {
        while let Some(copy) = self
            .output
            .instructions
            .windows(3)
            .position(direct_call_result_store)
            .map(|call| call + 1)
        {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[copy + 1] else {
                unreachable!("the direct result store was matched")
            };
            *s = Eabi::FIRST_GENERAL_ARGUMENT;
            self.remove_allocator_result_instruction(copy);
        }
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

#[derive(Clone, Copy)]
struct AllocatorInitializationPrefix {
    first_width: usize,
    first_shift: usize,
    pool_low: usize,
    constant_high: usize,
    constant_low: usize,
    compare: usize,
    object: u8,
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

fn allocator_initialization_prefix(
    output: &mwcc_machine_code::MachineFunction,
    result: u8,
) -> Option<AllocatorInitializationPrefix> {
    let first_width = output.instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadHalfwordZero {
                    d: width,
                    a: object,
                    offset: 8,
                },
                Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: width_source,
                    shift: 2,
                },
                Instruction::AddImmediate {
                    d: adjusted,
                    a: scaled_source,
                    immediate: 1,
                },
                Instruction::StoreHalfword {
                    s: stored,
                    a,
                    offset: 2,
                },
            ] if width == width_source
                && scaled == scaled_source
                && adjusted == stored
                && *a == result
                && *object != result
        )
    })?;
    let object = match output.instructions[first_width] {
        Instruction::LoadHalfwordZero { a, .. } => a,
        _ => unreachable!("the first width load was matched"),
    };
    let first_branch = output.instructions[first_width..]
        .iter()
        .position(is_result_schedule_barrier)
        .map(|offset| first_width + offset)?;
    let search_start = first_width.saturating_sub(4);
    let pool_low = (search_start..first_branch).find(|&index| {
        matches!(
            output.instructions[index],
            Instruction::LoadFloatSingle { offset: 0, .. }
        ) && output.relocations.iter().any(|relocation| {
            relocation.instruction_index == index && relocation.kind == RelocationKind::Addr16Lo
        })
    })?;
    let constant_high = (search_start..first_branch).find(|&index| {
        matches!(
            output.instructions[index],
            Instruction::AddImmediateShifted {
                a: 0,
                immediate: 1,
                ..
            }
        )
    })?;
    let constant_register = match output.instructions[constant_high] {
        Instruction::AddImmediateShifted { d, .. } => d,
        _ => unreachable!("the constant high half was matched"),
    };
    let constant_low = (constant_high + 1..first_branch).find(|&index| {
        matches!(
            output.instructions[index],
            Instruction::AddImmediate {
                a,
                immediate: -12,
                ..
            } if a == constant_register
        )
    })?;
    let compare = (search_start..first_branch).find(|&index| {
        matches!(
            output.instructions[index],
            Instruction::CompareWordImmediate { immediate: 0, .. }
        )
    })?;
    if output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                if (search_start..first_branch).contains(target)
        )
    }) {
        return None;
    }
    Some(AllocatorInitializationPrefix {
        first_width,
        first_shift: first_width + 1,
        pool_low,
        constant_high,
        constant_low,
        compare,
        object,
    })
}

fn assign_allocator_initialization_registers(
    instructions: &mut [Instruction],
    object: u8,
    result: u8,
) {
    for (load_offset, store_offset) in [(20, 4), (28, 12)] {
        if let Some(start) = instructions.windows(2).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord {
                        d: loaded,
                        a: 1,
                        offset,
                    },
                    Instruction::StoreHalfword {
                        s,
                        a,
                        offset: stored_offset,
                    },
                ] if loaded == s
                    && *offset == load_offset
                    && *a == result
                    && *stored_offset == store_offset
            )
        }) {
            let Instruction::LoadWord { d, .. } = &mut instructions[start] else {
                unreachable!()
            };
            *d = 3;
            let Instruction::StoreHalfword { s, .. } = &mut instructions[start + 1] else {
                unreachable!()
            };
            *s = 3;
        }
    }
    for (load_offset, store_offset) in [(8, 2), (10, 10)] {
        if let Some(start) = instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadHalfwordZero {
                        d: loaded,
                        a,
                        offset,
                    },
                    Instruction::ShiftLeftImmediate {
                        a: scaled,
                        s,
                        shift: 2,
                    },
                    Instruction::AddImmediate {
                        d: adjusted,
                        a: scaled_source,
                        immediate: 1,
                    },
                    Instruction::StoreHalfword {
                        s: stored,
                        a: store_base,
                        offset: stored_offset,
                    },
                ] if loaded == s
                    && scaled == scaled_source
                    && adjusted == stored
                    && *a == object
                    && *offset == load_offset
                    && *store_base == result
                    && *stored_offset == store_offset
            )
        }) {
            instructions[start] = Instruction::LoadHalfwordZero {
                d: if load_offset == 8 { 4 } else { 3 },
                a: object,
                offset: load_offset,
            };
            instructions[start + 1] = Instruction::ShiftLeftImmediate {
                a: 3,
                s: if load_offset == 8 { 4 } else { 3 },
                shift: 2,
            };
            instructions[start + 2] = Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            };
            instructions[start + 3] = Instruction::StoreHalfword {
                s: 3,
                a: result,
                offset: store_offset,
            };
        }
    }
    if let Some(start) = instructions.windows(2).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: loaded,
                    a,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s,
                    a: store_base,
                    offset: 16,
                },
            ] if loaded == s && *a == object && *store_base == result
        )
    }) {
        let Instruction::LoadWord { d, .. } = &mut instructions[start] else {
            unreachable!()
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut instructions[start + 1] else {
            unreachable!()
        };
        *s = 3;
    }
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

fn direct_call_result_store(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::BranchAndLink { .. },
            Instruction::Or {
                a: temporary,
                s: Eabi::FIRST_GENERAL_ARGUMENT,
                b: Eabi::FIRST_GENERAL_ARGUMENT,
            },
            Instruction::StoreWord {
                s,
                a: store_base,
                ..
            },
            ..
        ] if temporary == s && temporary != store_base
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

    #[test]
    fn recognizes_the_preallocation_scalar_initialization_prefix() {
        let mut output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::LoadHalfwordZero {
                    d: 0,
                    a: 33,
                    offset: 8,
                },
                Instruction::ShiftLeftImmediate {
                    a: 47,
                    s: 0,
                    shift: 2,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 47,
                    immediate: 1,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: 36,
                    offset: 2,
                },
                Instruction::LoadFloatSingle {
                    d: 48,
                    a: 4,
                    offset: 0,
                },
                Instruction::AddImmediateShifted {
                    d: 53,
                    a: 0,
                    immediate: 1,
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 53,
                    immediate: -12,
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: 36,
                    offset: 20,
                },
                Instruction::CompareWordImmediate {
                    a: 34,
                    immediate: 0,
                },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 10,
                },
            ],
            ..Default::default()
        };
        output.relocations.push(Relocation {
            instruction_index: 4,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::Constant(0),
        });
        let prefix = allocator_initialization_prefix(&output, 36).expect("the prefix should match");
        assert_eq!(
            (
                prefix.first_width,
                prefix.first_shift,
                prefix.pool_low,
                prefix.constant_high,
                prefix.constant_low,
                prefix.compare,
                prefix.object,
            ),
            (0, 1, 4, 5, 6, 8, 33)
        );
    }

    #[test]
    fn recognizes_an_immediately_stored_direct_call_result() {
        assert!(direct_call_result_store(&[
            Instruction::BranchAndLink {
                target: "get_data".into(),
            },
            Instruction::move_register(47, Eabi::FIRST_GENERAL_ARGUMENT),
            Instruction::StoreWord {
                s: 47,
                a: 38,
                offset: 4,
            },
        ]));
        assert!(!direct_call_result_store(&[
            Instruction::BranchAndLink {
                target: "get_data".into(),
            },
            Instruction::move_register(47, Eabi::FIRST_GENERAL_ARGUMENT),
            Instruction::StoreWord {
                s: 46,
                a: 38,
                offset: 4,
            },
        ]));
    }
}
