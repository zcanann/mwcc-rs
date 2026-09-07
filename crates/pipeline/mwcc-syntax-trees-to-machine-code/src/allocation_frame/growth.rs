//! Add save capacity at the top of a canonical predecrement frame.
//!
//! Locals and outgoing arguments keep their offsets from the allocated SP.
//! Saved GPRs move up; accesses into the caller's frame move by the same delta.
//! Validation precedes publication so unsupported stack addressing cannot leave
//! a partially resized instruction stream.

use mwcc_machine_code::Instruction;
use mwcc_vreg::{Class, RegisterRole};

pub(super) fn grow(
    instructions: &mut Vec<Instruction>,
    old_size: i16,
    declared_count: usize,
    required_count: usize,
    save_indices: &[usize],
    restore_indices: &[usize],
) -> Result<i16, &'static str> {
    let additional = required_count
        .checked_sub(declared_count)
        .and_then(|count| i16::try_from(count).ok())
        .and_then(|count| count.checked_mul(4))
        .ok_or("allocated GPR frame growth is out of range")?;
    let new_size = old_size
        .checked_add(additional)
        .and_then(|size| size.checked_add(15))
        .map(|size| size & !15)
        .filter(|size| size.checked_add(4).is_some())
        .ok_or("allocated GPR frame growth is out of range")?;
    if !matches!(instructions.first(),
        Some(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset }) if *offset == -old_size)
    {
        return Err("allocated GPR frame growth requires a canonical predecrement entry");
    }
    let delta = new_size - old_size;
    let mut resized = instructions.clone();
    for (index, instruction) in resized.iter_mut().enumerate() {
        if index == 0 {
            *instruction = Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -new_size,
            };
            continue;
        }
        if let Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate,
        } = instruction
        {
            if *immediate != old_size {
                return Err(
                    "allocated GPR frame growth encountered a noncanonical stack adjustment",
                );
            }
            *immediate = new_size;
            continue;
        }
        let touches_stack = mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| operand.class == Class::General && operand.register == 1);
        if !touches_stack {
            continue;
        }
        if mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| {
                operand.class == Class::General
                    && operand.register == 1
                    && operand.role == RegisterRole::Define
            })
        {
            return Err("allocated GPR frame growth encountered an untracked stack definition");
        }
        let offset = match instruction {
            Instruction::LoadWord { a: 1, offset, .. }
            | Instruction::LoadHalfwordZero { a: 1, offset, .. }
            | Instruction::LoadHalfwordAlgebraic { a: 1, offset, .. }
            | Instruction::LoadByteZero { a: 1, offset, .. }
            | Instruction::StoreWord { a: 1, offset, .. }
            | Instruction::StoreHalfword { a: 1, offset, .. }
            | Instruction::StoreByte { a: 1, offset, .. }
            | Instruction::LoadFloatSingle { a: 1, offset, .. }
            | Instruction::LoadFloatDouble { a: 1, offset, .. }
            | Instruction::StoreFloatSingle { a: 1, offset, .. }
            | Instruction::StoreFloatDouble { a: 1, offset, .. } => offset,
            Instruction::AddImmediate {
                a: 1, immediate, ..
            } => immediate,
            _ => return Err("allocated GPR frame growth encountered untracked stack addressing"),
        };
        if save_indices.contains(&index) || restore_indices.contains(&index) || *offset >= old_size
        {
            *offset = offset
                .checked_add(delta)
                .ok_or("allocated GPR frame displacement is out of range")?;
        }
    }
    *instructions = resized;
    Ok(new_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_saves_and_caller_arguments_without_moving_local_storage() {
        let mut code = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 32,
                a: 1,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 1,
                immediate: 8,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 32,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
        ];
        assert_eq!(grow(&mut code, 16, 1, 3, &[3], &[9]), Ok(32));
        assert_eq!(
            code[2],
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36
            }
        );
        assert_eq!(
            code[3],
            Instruction::StoreWord {
                s: 32,
                a: 1,
                offset: 28
            }
        );
        assert_eq!(
            code[4],
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8
            }
        );
        assert_eq!(
            code[5],
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 40
            }
        );
        assert_eq!(
            code[6],
            Instruction::AddImmediate {
                d: 5,
                a: 1,
                immediate: 8
            }
        );
        assert_eq!(
            code[7],
            Instruction::AddImmediate {
                d: 6,
                a: 1,
                immediate: 40
            }
        );
        assert_eq!(
            code[8],
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36
            }
        );
        assert_eq!(
            code[9],
            Instruction::LoadWord {
                d: 32,
                a: 1,
                offset: 28
            }
        );
        assert_eq!(
            code[10],
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32
            }
        );
    }

    #[test]
    fn leaves_unsupported_stack_addressing_untouched() {
        let mut code = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::LoadWordIndexed { d: 3, a: 1, b: 4 },
        ];
        let before = code.clone();
        assert!(grow(&mut code, 16, 1, 3, &[], &[]).is_err());
        assert_eq!(code, before);
    }
}
