//! Reuse of a narrow member load across packet-loop setup control flow.
//!
//! A packed-width expression can first consume a member through a
//! non-destructive `rlwinm`, then need the original halfword again after a
//! clamp diamond. When allocation leaves an otherwise idle lane for the later
//! load, preserve the first load there and remove the reload.

#[allow(unused_imports)]
use super::*;

pub(super) fn preserve_earlier_member_load(instructions: &mut [Instruction], setup: usize) -> bool {
    let Some(Instruction::LoadHalfwordZero {
        d: retained,
        a: setup_base,
        offset: setup_offset,
    }) = instructions.get(setup)
    else {
        return false;
    };
    let (retained, setup_base, setup_offset) = (*retained, *setup_base, *setup_offset);

    let search_start = setup.saturating_sub(32);
    for load in (search_start..setup).rev() {
        let Instruction::LoadHalfwordZero {
            d: original,
            a: base,
            offset,
        } = instructions[load]
        else {
            continue;
        };
        if base != setup_base || offset != setup_offset || original == retained || load + 3 >= setup
        {
            continue;
        }
        let Instruction::RotateAndMask {
            a: scaled,
            s: scale_source,
            shift: 1,
            begin: 15,
            end: 30,
        } = instructions[load + 2]
        else {
            continue;
        };
        let Instruction::DivideWordUnsigned { b: divisor, .. } = instructions[load + 3] else {
            continue;
        };
        if scale_source != original || divisor != scaled || scaled == retained {
            continue;
        }

        let safe_interval = instructions[load + 1..setup].iter().all(|instruction| {
            general_access(instruction, retained).is_some_and(|(reads, writes)| !reads && !writes)
                && general_access(instruction, setup_base).is_some_and(|(_, writes)| !writes)
        });
        if !safe_interval {
            continue;
        }

        let Instruction::LoadHalfwordZero { d, .. } = &mut instructions[load] else {
            unreachable!()
        };
        *d = retained;
        let Instruction::RotateAndMask { s, .. } = &mut instructions[load + 2] else {
            unreachable!()
        };
        *s = retained;
        return true;
    }
    false
}

/// Register access for the straight-line integer subset allowed between the
/// source load and packet setup. Unknown instructions reject the rewrite.
fn general_access(instruction: &Instruction, register: u8) -> Option<(bool, bool)> {
    use Instruction::*;
    let access = match instruction {
        AddImmediate { d, a, .. } | AddImmediateShifted { d, a, .. } => {
            (*a == register && *a != 0, *d == register)
        }
        RotateAndMask { a, s, .. } | ShiftLeftImmediate { a, s, .. } => {
            (*s == register, *a == register)
        }
        DivideWordUnsigned { d, a, b } | Add { d, a, b } | SubtractFrom { d, a, b } => {
            (*a == register || *b == register, *d == register)
        }
        CompareWordImmediate { a, .. } => (*a == register, false),
        Negate { d, a } => (*a == register, *d == register),
        Or { a, s, b } => (*s == register || *b == register, *a == register),
        LoadHalfwordZero { d, a, .. } => (*a == register, *d == register),
        BranchConditionalForward { .. } | Branch { .. } => (false, false),
        _ => return None,
    };
    Some(access)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence() -> Vec<Instruction> {
        vec![
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 26,
                offset: 4,
            },
            Instruction::load_immediate(4, 4096),
            Instruction::RotateAndMask {
                a: 0,
                s: 0,
                shift: 1,
                begin: 15,
                end: 30,
            },
            Instruction::DivideWordUnsigned { d: 4, a: 4, b: 0 },
            Instruction::CompareWordImmediate {
                a: 15,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 8,
            },
            Instruction::Negate { d: 3, a: 15 },
            Instruction::Branch { target: 9 },
            Instruction::load_immediate(3, 0),
            Instruction::Or { a: 6, s: 15, b: 15 },
            Instruction::LoadHalfwordZero {
                d: 10,
                a: 26,
                offset: 4,
            },
        ]
    }

    #[test]
    fn preserves_the_raw_halfword_in_the_later_load_lane() {
        let mut instructions = sequence();
        assert!(preserve_earlier_member_load(&mut instructions, 10));
        assert!(matches!(
            instructions[0],
            Instruction::LoadHalfwordZero { d: 10, .. }
        ));
        assert!(matches!(
            instructions[2],
            Instruction::RotateAndMask { a: 0, s: 10, .. }
        ));
    }

    #[test]
    fn rejects_a_lane_touched_across_the_control_flow() {
        let mut instructions = sequence();
        instructions.insert(6, Instruction::load_immediate(10, 1));
        assert!(!preserve_earlier_member_load(&mut instructions, 11));
    }
}
