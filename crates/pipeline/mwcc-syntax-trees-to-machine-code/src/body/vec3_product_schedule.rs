//! Final physical schedule for the legacy inline three-float product.
//!
//! Semantic lowering exposes the operator's scratch and return objects before
//! frame planning. Generic scalar emission deliberately retains source order;
//! this pass recognizes that complete physical packet and applies MWCC's
//! cross-lane issue order, pointer reuse, and linkage-first prologue spelling.

use crate::{Generator, move_instruction_before_retargeting, remove_instruction_retargeting_to_next};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Function};
use mwcc_vreg::{Class, RegisterRole, for_each_register, register_operands};

impl Generator {
    pub(crate) fn schedule_materialized_vec3_product(&mut self, function: &Function) {
        if !crate::vec3_product_temporaries::owns_materialized_frame(function) {
            return;
        }
        let Some(scratch_offset) = self
            .frame_slots
            .iter()
            .find_map(|(name, slot)| name.starts_with("__mwcc_vec3_scratch").then_some(slot.offset))
        else {
            return;
        };
        let Some(result_offset) = self
            .frame_slots
            .iter()
            .find_map(|(name, slot)| name.starts_with("__mwcc_vec3_result").then_some(slot.offset))
        else {
            return;
        };

        normalize_linkage_first_prologue(&mut self.output.instructions, self.frame_size);
        let Some(packet_start) = schedule_float_packet(
            &mut self.output.instructions,
            scratch_offset,
            result_offset,
        ) else {
            return;
        };

        self.reuse_guarded_product_destination(packet_start, result_offset);
        self.coalesce_adjacent_product_member_loads();
        if matches!(
            function.return_expression,
            Some(Expression::IntegerLiteral(0))
        ) {
            self.schedule_product_integer_return(packet_start);
        }
    }

    fn reuse_guarded_product_destination(&mut self, packet_start: usize, result_offset: i16) {
        let target_load = self
            .output
            .instructions
            .windows(2)
            .enumerate()
            .skip(packet_start + 12)
            .find_map(|(index, pair)| match (&pair[0], &pair[1]) {
                (
                    Instruction::LoadWord {
                        d: target,
                        a: root,
                        offset,
                    },
                    Instruction::LoadWord {
                        d: 4,
                        a: 1,
                        offset: loaded_result,
                    },
                ) if *loaded_result == result_offset => Some((index, *target, *root, *offset)),
                _ => None,
            });
        let Some((target_load, target, root, offset)) = target_load else {
            return;
        };
        let Some((earlier, old_target)) = self.output.instructions[..packet_start]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, instruction)| match instruction {
                Instruction::LoadWord { d, a, offset: candidate }
                    if *a == root && *candidate == offset && *d != target =>
                {
                    Some((index, *d))
                }
                _ => None,
            })
        else {
            return;
        };
        if !replace_general_uses(
            &mut self.output.instructions[earlier + 1..target_load],
            old_target,
            target,
        ) {
            return;
        }
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[earlier] else {
            unreachable!("the guarded destination load was matched")
        };
        *d = target;
        remove_instruction_retargeting_to_next(self, target_load);
    }

    fn coalesce_adjacent_product_member_loads(&mut self) {
        let mut index = 0usize;
        while index + 1 < self.output.instructions.len() {
            let pair = match (
                &self.output.instructions[index],
                &self.output.instructions[index + 1],
            ) {
                (
                    Instruction::LoadWord {
                        d: first,
                        a: first_base,
                        offset: first_offset,
                    },
                    Instruction::LoadWord {
                        d: second,
                        a: second_base,
                        offset: second_offset,
                    },
                ) if first != second
                    && first_base == second_base
                    && first_offset == second_offset =>
                {
                    Some((*first, *second))
                }
                _ => None,
            };
            let Some((first, second)) = pair else {
                index += 1;
                continue;
            };
            let (discarded, retained, remove) = if first < second {
                (second, first, index + 1)
            } else {
                (first, second, index)
            };
            if !replace_general_uses(
                &mut self.output.instructions[index + 2..],
                discarded,
                retained,
            ) {
                index += 1;
                continue;
            }
            remove_instruction_retargeting_to_next(self, remove);
        }
    }

    fn schedule_product_integer_return(&mut self, packet_start: usize) {
        let multiply = packet_start + 9;
        if !matches!(
            self.output.instructions.get(multiply),
            Some(Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 })
        ) {
            return;
        }
        let Some(result) = self
            .output
            .instructions
            .iter()
            .enumerate()
            .skip(packet_start + 12)
            .find_map(|(index, instruction)| match instruction {
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 0,
                } => Some(index),
                _ => None,
            })
        else {
            return;
        };
        if result > multiply {
            move_instruction_before_retargeting(self, result, multiply);
        }
    }
}

fn normalize_linkage_first_prologue(instructions: &mut [Instruction], frame_size: i16) {
    let [
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: push,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: link,
        },
        ..
    ] = instructions
    else {
        return;
    };
    if *push != -frame_size || *link != frame_size + 4 {
        return;
    }
    instructions[..3].clone_from_slice(&[
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -frame_size,
        },
    ]);
}

fn schedule_float_packet(
    instructions: &mut [Instruction],
    scratch_offset: i16,
    result_offset: i16,
) -> Option<usize> {
    for start in 0..instructions.len().checked_sub(11)? {
        let (
            scale_base,
            scale_offset,
            source_base,
            source_offset,
        ) = match &instructions[start..start + 12] {
            [
                Instruction::LoadFloatSingle {
                    d: scale,
                    a: scale_base,
                    offset: scale_offset,
                },
                Instruction::LoadFloatSingle {
                    d: x,
                    a: source_base,
                    offset: source_offset,
                },
                Instruction::FloatMultiplySingle { d, a, c },
                Instruction::StoreFloatSingle {
                    s,
                    a: 1,
                    offset: scratch,
                },
                Instruction::LoadFloatSingle {
                    d: reloaded,
                    a: 1,
                    offset: scratch_load,
                },
                Instruction::StoreFloatSingle {
                    s: copied,
                    a: 1,
                    offset: result_x,
                },
                Instruction::LoadFloatSingle {
                    d: y,
                    a: y_base,
                    offset: y_offset,
                },
                Instruction::FloatMultiplySingle {
                    d: y_product,
                    a: y_operand,
                    c: y_scale,
                },
                Instruction::StoreFloatSingle {
                    s: stored_y,
                    a: 1,
                    offset: result_y,
                },
                Instruction::LoadFloatSingle {
                    d: z,
                    a: z_base,
                    offset: z_offset,
                },
                Instruction::FloatMultiplySingle {
                    d: z_product,
                    a: z_operand,
                    c: z_scale,
                },
                Instruction::StoreFloatSingle {
                    s: stored_z,
                    a: 1,
                    offset: result_z,
                },
            ] if d == x
                && a == x
                && c == scale
                && s == x
                && *scratch == scratch_offset
                && reloaded == x
                && *scratch_load == scratch_offset
                && copied == x
                && *result_x == result_offset
                && y == x
                && y_base == source_base
                && *y_offset == source_offset.checked_add(4)?
                && y_product == x
                && y_operand == x
                && y_scale == scale
                && stored_y == x
                && *result_y == result_offset.checked_add(4)?
                && z == x
                && z_base == source_base
                && *z_offset == source_offset.checked_add(8)?
                && z_product == x
                && z_operand == x
                && z_scale == scale
                && stored_z == x
                && *result_z == result_offset.checked_add(8)? =>
            {
                (*scale_base, *scale_offset, *source_base, *source_offset)
            }
            _ => continue,
        };
        instructions[start..start + 12].clone_from_slice(&[
            Instruction::LoadFloatSingle {
                d: 2,
                a: scale_base,
                offset: scale_offset,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: source_base,
                offset: source_offset,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: source_base,
                offset: source_offset.checked_add(8)?,
            },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 2 },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: scratch_offset,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 1,
                offset: scratch_offset,
            },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: result_offset,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: source_base,
                offset: source_offset.checked_add(4)?,
            },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: result_offset.checked_add(4)?,
            },
            Instruction::StoreFloatSingle {
                s: 1,
                a: 1,
                offset: result_offset.checked_add(8)?,
            },
        ]);
        return Some(start);
    }
    None
}

/// Replace reads of one physical GPR until its next definition. The return
/// value proves the discarded definition was actually consumed.
fn replace_general_uses(instructions: &mut [Instruction], old: u8, new: u8) -> bool {
    let mut replaced = false;
    for instruction in instructions {
        let redefined = register_operands(instruction).iter().any(|operand| {
            operand.class == Class::General
                && operand.role == RegisterRole::Define
                && operand.register == old
        });
        for_each_register(instruction, |role, class, register| {
            if role == RegisterRole::Use && class == Class::General && *register == old {
                *register = new;
                replaced = true;
            }
        });
        if redefined {
            break;
        }
    }
    replaced
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_the_three_float_packet() {
        let mut instructions = vec![
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 20 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 24 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 24 },
            Instruction::LoadFloatSingle { d: 0, a: 1, offset: 24 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 32 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 28 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 36 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 32 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 40 },
        ];

        assert_eq!(schedule_float_packet(&mut instructions, 24, 32), Some(0));
        assert!(matches!(
            instructions[2],
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 32 }
        ));
        assert!(matches!(
            instructions[4],
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 2 }
        ));
    }
}
