//! State-diamond homes and loop-entry issue order for dense counted loops.
//!
//! The source initializes or reloads six carried encoder values, then clears
//! four per-iteration flags. Generic lowering shares one zero definition among
//! those flags; MWCC materializes independent zeroes and schedules the sample
//! load first. Recovering that semantic packet before allocation also exposes
//! MWCC's intended long-lived state homes without post-allocation renaming.

#[allow(unused_imports)]
use super::*;

const STATE_HOMES: [u8; 6] = [11, 12, 5, 27, 26, 25];
const LOOP_ORDER: [usize; 9] = [4, 1, 2, 3, 6, 0, 5, 7, 8];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StatePlan {
    values: [u8; 6],
    loop_start: usize,
    flags: [u8; 4],
    sample: u8,
}

impl Generator {
    pub(crate) fn schedule_dense_counted_loop_state(&mut self) -> bool {
        if !self.structured_dense_counted_loop_entry_owner {
            return false;
        }
        let Some(plan) = locate_state_plan(&self.output.instructions) else {
            return false;
        };
        for (value, home) in plan.values.into_iter().zip(STATE_HOMES) {
            self.prefer_virtual_general(value, home);
        }
        for (flag, home) in plan.flags.into_iter().zip([7, 10, 9, 8]) {
            self.prefer_virtual_general(flag, home);
        }
        if !merge_sample_into_overwritten_state(
            &mut self.output.instructions,
            plan.loop_start + 4,
            plan.sample,
            plan.values[2],
        ) {
            return false;
        }
        let Some((delta, n_dlx)) = rewrite_state_arithmetic(
            &mut self.output.instructions,
            plan.loop_start + 9,
            plan.values,
        ) else {
            return false;
        };
        self.prefer_virtual_general(delta, 19);
        self.prefer_virtual_general(n_dlx, 19);
        if !rewrite_state_product(self, plan.loop_start + 31, plan.values, plan.flags, n_dlx) {
            return false;
        }
        if !rewrite_state_publication(&mut self.output.instructions, plan.values) {
            return false;
        }

        let mut old = self.output.instructions[plan.loop_start..plan.loop_start + 9].to_vec();
        for relative in 1..=3 {
            let Instruction::Or { a, .. } = old[relative] else {
                unreachable!("the dense counted flag was recognized as a zero copy")
            };
            old[relative] = Instruction::load_immediate(a, 0);
        }
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in LOOP_ORDER.into_iter().enumerate() {
            self.output.instructions[plan.loop_start + new_relative] = old[old_relative].clone();
            permutation[plan.loop_start + old_relative] = plan.loop_start + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        crate::retarget_instruction_destinations(self, plan.loop_start + 5, plan.loop_start);
        if !merge_integer_conversion_state(self, plan.values[1]) {
            return false;
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }
}

fn merge_integer_conversion_state(generator: &mut Generator, n_dl: u8) -> bool {
    let Some(copy) = generator.output.instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::Or { a, s, b } if a != s && s == &n_dl && b == &n_dl)
    }) else {
        return false;
    };
    let Some(xoris) = (copy + 1..copy.saturating_add(8).min(generator.output.instructions.len()))
        .find(|&index| {
            matches!(
                generator.output.instructions[index],
                Instruction::XorImmediateShifted { a, s, immediate: 32768 }
                    if a == s
                        && matches!(generator.output.instructions[copy], Instruction::Or { a: copied, .. } if copied == a)
            )
        })
    else {
        return false;
    };
    let copied = match generator.output.instructions[copy] {
        Instruction::Or { a, .. } => a,
        _ => unreachable!(),
    };
    let Some(store) = (xoris + 1..xoris.saturating_add(4).min(generator.output.instructions.len()))
        .find(|&index| {
            matches!(
                generator.output.instructions[index],
                Instruction::StoreWord { s, a: 1, offset: 76 } if s == copied
            )
        })
    else {
        return false;
    };
    generator.output.instructions[xoris] = Instruction::XorImmediateShifted {
        a: n_dl,
        s: n_dl,
        immediate: 32768,
    };
    let Instruction::StoreWord { s, .. } = &mut generator.output.instructions[store] else {
        unreachable!("the dense integer conversion was recognized as a word store")
    };
    *s = n_dl;
    crate::remove_instruction_retargeting_to_next(generator, copy);
    true
}

fn rewrite_state_product(
    generator: &mut Generator,
    start: usize,
    values: [u8; 6],
    flags: [u8; 4],
    n_dlx: u8,
) -> bool {
    let Some(window) = generator.output.instructions.get(start..start + 12) else {
        return false;
    };
    let Instruction::ShiftLeftImmediate {
        a: factor,
        s,
        shift: 1,
    } = window[3]
    else {
        return false;
    };
    let Instruction::MultiplyLow {
        d: term_a,
        a: term_a_source,
        b: term_a_flag,
    } = window[5]
    else {
        return false;
    };
    let Instruction::MultiplyLow {
        d: term_c,
        a: term_c_source,
        b: term_c_flag,
    } = window[8]
    else {
        return false;
    };
    if s != flags[0]
        || term_a_source != values[1]
        || term_a_flag != flags[3]
        || term_c_source != values[5]
        || term_c_flag != flags[1]
        || !matches!(window[0], Instruction::ShiftRightLogicalImmediate { a, s, shift: 31 } if a == n_dlx && s == values[5])
        || !matches!(window[1], Instruction::Add { d, a, b } if d == n_dlx && a == n_dlx && b == values[5])
        || !matches!(window[2], Instruction::ShiftRightAlgebraicImmediate { a, s, shift: 1 } if a == n_dlx && s == n_dlx)
        || !matches!(window[4], Instruction::SubtractFromImmediate { d, a, immediate: 1 } if d == factor && a == factor)
        || !matches!(window[6], Instruction::MultiplyLow { d: 0, a, b } if a == values[4] && b == flags[2])
        || !matches!(window[7], Instruction::Add { d: 0, a, b: 0 } if a == term_a)
        || !matches!(window[9], Instruction::Add { d: 0, a, b: 0 } if a == term_c)
        || !matches!(window[10], Instruction::Add { d: 0, a, b: 0 } if a == n_dlx)
        || !matches!(window[11], Instruction::MultiplyLow { d, a, b: 0 } if d == values[2] && a == factor)
    {
        return false;
    }

    let term_b = generator.fresh_virtual_general_preferring(22);
    generator.prefer_virtual_general(term_a, 21);
    generator.prefer_virtual_general(term_c, 20);
    generator.output.instructions[start + 3] = Instruction::ShiftLeftImmediate {
        a: values[2],
        s: flags[0],
        shift: 1,
    };
    generator.output.instructions[start + 4] = Instruction::SubtractFromImmediate {
        d: values[2],
        a: values[2],
        immediate: 1,
    };
    generator.output.instructions[start + 6] = Instruction::MultiplyLow {
        d: term_b,
        a: values[4],
        b: flags[2],
    };
    generator.output.instructions[start + 7] = Instruction::Add {
        d: term_a,
        a: term_c,
        b: term_a,
    };
    generator.output.instructions[start + 9] = Instruction::Add {
        d: term_b,
        a: n_dlx,
        b: term_b,
    };
    generator.output.instructions[start + 10] = Instruction::Add {
        d: term_b,
        a: term_a,
        b: term_b,
    };
    generator.output.instructions[start + 11] = Instruction::MultiplyLow {
        d: values[2],
        a: values[2],
        b: term_b,
    };

    let order = [8, 3, 0, 4, 1, 5, 2, 6, 7, 9, 10, 11];
    let old = generator.output.instructions[start..start + 12].to_vec();
    let mut permutation: Vec<usize> = (0..generator.output.instructions.len()).collect();
    for (new_relative, old_relative) in order.into_iter().enumerate() {
        generator.output.instructions[start + new_relative] = old[old_relative].clone();
        permutation[start + old_relative] = start + new_relative;
    }
    crate::remap_instruction_indices(generator, &permutation);
    crate::retarget_instruction_destinations(generator, start + 2, start);
    generator
        .output
        .relocations
        .sort_by_key(|relocation| relocation.instruction_index);
    true
}

fn rewrite_state_publication(instructions: &mut [Instruction], values: [u8; 6]) -> bool {
    let Some(start) = instructions.windows(6).position(|window| {
        let mut base = None;
        for (index, instruction) in window.iter().enumerate() {
            let Instruction::StoreWord { s, a, offset } = instruction else {
                return false;
            };
            let expected_source = if index == 3 { 3 } else { values[index] };
            if *s != expected_source || *offset != (index * 4) as i16 {
                return false;
            }
            if let Some(base) = base {
                if *a != base {
                    return false;
                }
            } else {
                base = Some(*a);
            }
        }
        true
    }) else {
        return false;
    };
    let Instruction::StoreWord { s, .. } = &mut instructions[start + 3] else {
        unreachable!("the dense state publication was recognized as a word store")
    };
    *s = values[3];
    true
}

fn rewrite_state_arithmetic(
    instructions: &mut [Instruction],
    start: usize,
    values: [u8; 6],
) -> Option<(u8, u8)> {
    let window = instructions.get(start..start + 25)?;
    let Instruction::SubtractFrom {
        d: delta,
        a,
        b,
    } = window[0]
    else {
        return None;
    };
    let Instruction::ShiftRightAlgebraicImmediate {
        a: sign,
        s,
        shift: 31,
    } = window[1]
    else {
        return None;
    };
    let Instruction::ShiftRightAlgebraicImmediate {
        a: n_dlh,
        s: 0,
        shift: 1,
    } = window[10]
    else {
        return None;
    };
    let Instruction::ShiftRightAlgebraicImmediate {
        a: n_dlq,
        s: 0,
        shift: 1,
    } = window[17]
    else {
        return None;
    };
    let Instruction::ShiftRightAlgebraicImmediate {
        a: n_dlx,
        s: 0,
        shift: 1,
    } = window[24]
    else {
        return None;
    };
    if a != values[0]
        || b != values[2]
        || s != delta
        || n_dlh != values[4]
        || n_dlq != values[5]
        || !matches!(window[2], Instruction::Xor { a: 3, s, b } if s == sign && b == delta)
        || !matches!(window[3], Instruction::SubtractFrom { d: 3, a, b: 3 } if a == sign)
        || !matches!(window[4], Instruction::CompareWord { a: 3, b } if b == values[1])
        || !matches!(window[7], Instruction::SubtractFrom { d: 3, a, b: 3 } if a == values[1])
        || !matches!(window[8], Instruction::ShiftRightLogicalImmediate { a: 0, s, shift: 31 } if s == values[1])
        || !matches!(window[9], Instruction::Add { d: 0, a: 0, b } if b == values[1])
        || !matches!(window[11], Instruction::CompareWord { a: 3, b } if b == values[4])
        || !matches!(window[14], Instruction::SubtractFrom { d: 3, a, b: 3 } if a == values[4])
        || !matches!(window[15], Instruction::ShiftRightLogicalImmediate { a: 0, s, shift: 31 } if s == values[4])
        || !matches!(window[16], Instruction::Add { d: 0, a: 0, b } if b == values[4])
        || !matches!(window[18], Instruction::CompareWord { a: 3, b } if b == values[5])
        || !matches!(window[21], Instruction::SubtractFrom { d: 3, a, b: 3 } if a == values[5])
        || !matches!(window[22], Instruction::ShiftRightLogicalImmediate { a: 0, s, shift: 31 } if s == values[5])
        || !matches!(window[23], Instruction::Add { d: 0, a: 0, b } if b == values[5])
    {
        return None;
    }

    let n_qn = values[2];
    let n_dn = values[3];
    instructions[start + 1] = Instruction::ShiftRightAlgebraicImmediate {
        a: n_qn,
        s: delta,
        shift: 31,
    };
    instructions[start + 2] = Instruction::Xor {
        a: n_dn,
        s: n_qn,
        b: delta,
    };
    instructions[start + 3] = Instruction::SubtractFrom {
        d: n_dn,
        a: n_qn,
        b: n_dn,
    };
    for relative in [4, 7, 11, 14, 18, 21] {
        mwcc_vreg::for_each_register(&mut instructions[start + relative], |_, class, register| {
            if class == mwcc_vreg::Class::General && *register == 3 {
                *register = n_dn;
            }
        });
    }
    for relative in [8, 9, 10, 15, 16, 17] {
        mwcc_vreg::for_each_register(&mut instructions[start + relative], |_, class, register| {
            if class == mwcc_vreg::Class::General && *register == 0 {
                *register = n_qn;
            }
        });
    }
    for relative in [22, 23, 24] {
        mwcc_vreg::for_each_register(&mut instructions[start + relative], |_, class, register| {
            if class == mwcc_vreg::Class::General && *register == 0 {
                *register = n_dlx;
            }
        });
    }
    Some((delta, n_dlx))
}

fn merge_sample_into_overwritten_state(
    instructions: &mut [Instruction],
    definition: usize,
    sample: u8,
    carried: u8,
) -> bool {
    let Some(Instruction::LoadHalfwordAlgebraic { d, .. }) = instructions.get_mut(definition)
    else {
        return false;
    };
    if *d != sample {
        return false;
    }
    *d = carried;
    let mut replaced = false;
    for instruction in &mut instructions[definition + 1..] {
        let redefines_carried = mwcc_vreg::register_operands(instruction).iter().any(|operand| {
            operand.class == mwcc_vreg::Class::General
                && operand.role == mwcc_vreg::RegisterRole::Define
                && operand.register == carried
        });
        mwcc_vreg::for_each_register(instruction, |role, class, register| {
            if role == mwcc_vreg::RegisterRole::Use
                && class == mwcc_vreg::Class::General
                && *register == sample
            {
                *register = carried;
                replaced = true;
            }
        });
        if redefines_carried {
            break;
        }
    }
    replaced
}

fn locate_state_plan(instructions: &[Instruction]) -> Option<StatePlan> {
    let values = instructions
        .windows(15)
        .enumerate()
        .find_map(|(start, window)| recognize_state_diamond(window, start))?;
    let (loop_start, flags, sample) = instructions
        .windows(9)
        .enumerate()
        .find_map(|(start, window)| {
            recognize_loop_entry(window, start, values[0])
                .map(|(flags, sample)| (start, flags, sample))
        })?;
    Some(StatePlan {
        values,
        loop_start,
        flags,
        sample,
    })
}

fn recognize_state_diamond(window: &[Instruction], start: usize) -> Option<[u8; 6]> {
    if !matches!(window[0], Instruction::AndMaskRecord { .. })
        || !matches!(
            window[1],
            Instruction::BranchConditionalForward { target, .. } if target == start + 9
        )
        || !matches!(window[8], Instruction::Branch { target } if target > start + 14)
    {
        return None;
    }
    let immediates = [0, 127, 0, 0, 0, 0];
    let mut values = [0u8; 6];
    for index in 0..6 {
        let Instruction::AddImmediate { d, a: 0, immediate } = window[index + 2] else {
            return None;
        };
        if immediate != immediates[index] {
            return None;
        }
        values[index] = d;
    }
    let mut base = None;
    for (index, &value) in values.iter().enumerate() {
        let Instruction::LoadWord { d, a, offset } = window[index + 9] else {
            return None;
        };
        if d != value || offset != i16::try_from(index * 4).ok()? {
            return None;
        }
        if let Some(base) = base {
            if a != base {
                return None;
            }
        } else {
            base = Some(a);
        }
    }
    Some(values)
}

fn recognize_loop_entry(
    window: &[Instruction],
    start: usize,
    n_xn: u8,
) -> Option<([u8; 4], u8)> {
    let Instruction::AddImmediate {
        d: first_flag,
        a: 0,
        immediate: 0,
    } = window[0]
    else {
        return None;
    };
    let mut flags = [first_flag, 0, 0, 0];
    for relative in 1..=3 {
        let Instruction::Or { a, s, b } = window[relative] else {
            return None;
        };
        if s != first_flag || b != first_flag {
            return None;
        }
        flags[relative] = a;
    }
    let Instruction::LoadHalfwordAlgebraic {
        d: sample,
        a: source,
        offset: 0,
    } = window[4]
    else {
        return None;
    };
    if !matches!(
        window[5],
        Instruction::AddImmediate { d, a, immediate: 2 } if d == source && a == source
    ) || !matches!(
        window[6],
        Instruction::CompareWord { a, b } if a == sample && b == n_xn
    ) || !matches!(
        window[7],
        Instruction::BranchConditionalForward { target, .. } if target == start + 9
    ) || !matches!(
        window[8],
        Instruction::AddImmediate { d, a: 0, immediate: 1 } if d == first_flag
    ) {
        return None;
    }
    Some((flags, sample))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_diamond() -> Vec<Instruction> {
        let values = [40, 41, 42, 43, 44, 45];
        let mut instructions = vec![
            Instruction::AndMaskRecord {
                a: 0,
                s: 19,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 9,
            },
        ];
        for (value, immediate) in values.into_iter().zip([0, 127, 0, 0, 0, 0]) {
            instructions.push(Instruction::AddImmediate {
                d: value,
                a: 0,
                immediate,
            });
        }
        instructions.push(Instruction::Branch { target: 20 });
        for (index, value) in values.into_iter().enumerate() {
            instructions.push(Instruction::LoadWord {
                d: value,
                a: 28,
                offset: (index * 4) as i16,
            });
        }
        instructions
    }

    fn loop_entry() -> Vec<Instruction> {
        vec![
            Instruction::load_immediate(50, 0),
            Instruction::move_register(51, 50),
            Instruction::move_register(52, 50),
            Instruction::move_register(53, 50),
            Instruction::LoadHalfwordAlgebraic {
                d: 54,
                a: 29,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 29,
                immediate: 2,
            },
            Instruction::CompareWord { a: 54, b: 40 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 9,
            },
            Instruction::load_immediate(50, 1),
        ]
    }

    #[test]
    fn recognizes_the_state_merge_and_iteration_flags() {
        assert_eq!(
            recognize_state_diamond(&state_diamond(), 0),
            Some([40, 41, 42, 43, 44, 45])
        );
        assert_eq!(
            recognize_loop_entry(&loop_entry(), 0, 40),
            Some(([50, 51, 52, 53], 54))
        );
    }

    #[test]
    fn rejects_a_state_load_from_another_block() {
        let mut instructions = state_diamond();
        let Instruction::LoadWord { a, .. } = &mut instructions[12] else {
            unreachable!()
        };
        *a = 27;
        assert_eq!(recognize_state_diamond(&instructions, 0), None);
    }

    #[test]
    fn issues_the_sample_before_independent_flag_zeroes() {
        assert_eq!(LOOP_ORDER, [4, 1, 2, 3, 6, 0, 5, 7, 8]);
    }

    #[test]
    fn merges_sample_reads_until_the_carried_state_is_redefined() {
        let mut instructions = vec![
            Instruction::LoadHalfwordAlgebraic {
                d: 54,
                a: 29,
                offset: 0,
            },
            Instruction::CompareWord { a: 54, b: 40 },
            Instruction::AddImmediate {
                d: 42,
                a: 54,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 54,
                a: 54,
                immediate: 2,
            },
        ];

        assert!(merge_sample_into_overwritten_state(
            &mut instructions,
            0,
            54,
            42
        ));
        assert!(matches!(
            instructions[0],
            Instruction::LoadHalfwordAlgebraic { d: 42, .. }
        ));
        assert_eq!(instructions[1], Instruction::CompareWord { a: 42, b: 40 });
        assert!(matches!(instructions[2], Instruction::AddImmediate { d: 42, a: 42, .. }));
        assert!(matches!(instructions[3], Instruction::AddImmediate { d: 54, a: 54, .. }));
    }
}
