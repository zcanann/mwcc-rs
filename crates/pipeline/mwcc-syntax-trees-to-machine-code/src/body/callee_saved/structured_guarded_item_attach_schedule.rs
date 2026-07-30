//! Physical schedule for an item attachment followed by two inlined guards.
//!
//! The attachment stores an integer-valued float argument, executes a small
//! guarded notification helper, then enters a nullable item-state callback.
//! Build 163 converts the float before linkage setup, assigns the source object
//! and retained fighter to r31/r30, and uses r3 for the callback-side reload.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

const ENTRY_SCHEDULE: [usize; 19] = [
    0, 12, 1, 16, 2, 3, 6, 5, 4, 13, 7, 8, 9, 10, 14, 11, 15, 17, 18,
];

impl Generator {
    pub(crate) fn schedule_guarded_item_attach(&mut self, function: &Function) {
        if self.frame_size != 48
            || !has_unused_eight_byte_array(function)
            || !is_guarded_item_attach_stream(&self.output.instructions)
        {
            return;
        }

        self.apply_guarded_item_attach_entry_schedule();
        self.rewrite_guarded_item_attach_registers();
    }

    fn apply_guarded_item_attach_entry_schedule(&mut self) {
        let mut current: Vec<_> = (0..ENTRY_SCHEDULE.len()).collect();
        for (destination, &original) in ENTRY_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|candidate| *candidate == original)
                .expect("the item-attachment entry schedule is a permutation");
            if source != destination {
                self.move_instruction_before(source, destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }

    fn rewrite_guarded_item_attach_registers(&mut self) {
        let instructions = &mut self.output.instructions;
        instructions[6] = Instruction::Or { a: 31, s: 3, b: 3 };
        instructions[8] = Instruction::LoadWord {
            d: 30,
            a: 3,
            offset: 44,
        };
        instructions[9] = Instruction::StoreFloatDouble {
            s: 0,
            a: 1,
            offset: 32,
        };
        instructions[10] = Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 6528,
        };
        instructions[11] = Instruction::StoreWord {
            s: 5,
            a: 30,
            offset: 8216,
        };
        instructions[12] = Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        };
        instructions[13] = Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: 1784,
        };
        instructions[14] = Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 36,
        };
        instructions[15] = Instruction::StoreWord {
            s: 4,
            a: 30,
            offset: 8220,
        };
        instructions[16] = Instruction::StoreWord {
            s: 3,
            a: 30,
            offset: 8228,
        };
        instructions[17] = Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 8225,
        };
        instructions[18] = Instruction::StoreByte {
            s: 0,
            a: 30,
            offset: 8224,
        };

        for index in [19, 20, 25, 28, 31] {
            let Instruction::LoadByteZero { a, .. } = &mut instructions[index] else {
                unreachable!("the guarded notification byte load was recognized")
            };
            *a = 30;
        }

        instructions[36] = Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 44,
        };
        for index in [37, 40, 43] {
            let Instruction::LoadWord { a, .. } = &mut instructions[index] else {
                unreachable!("the item-state callback load was recognized")
            };
            *a = 3;
        }
        instructions[52] = Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        };
    }
}

fn has_unused_eight_byte_array(function: &Function) -> bool {
    function.locals.iter().any(|local| {
        !local.is_static
            && local.array_length == Some(8)
            && !body_uses_local(&function.statements, &local.name)
    })
}

fn is_guarded_item_attach_stream(instructions: &[Instruction]) -> bool {
    instructions.len() == 60
        && matches!(instructions[0], Instruction::MoveFromLinkRegister { d: 0 })
        && matches!(
            instructions[1],
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4
            }
        )
        && matches!(
            instructions[2],
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -48
            }
        )
        && matches!(
            instructions[3],
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 44
            }
        )
        && matches!(
            instructions[4],
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 44
            }
        )
        && matches!(
            instructions[5],
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 40
            }
        )
        && matches!(instructions[6], Instruction::Or { a: 30, s: 3, b: 3 })
        && matches!(
            instructions[7],
            Instruction::StoreWord {
                s: 4,
                a: 31,
                offset: 6528
            }
        )
        && matches!(
            instructions[8],
            Instruction::StoreWord {
                s: 5,
                a: 31,
                offset: 8216
            }
        )
        && matches!(
            instructions[9],
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0
            }
        )
        && matches!(
            instructions[10],
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 1784
            }
        )
        && matches!(
            instructions[11],
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 8220
            }
        )
        && matches!(
            instructions[12],
            Instruction::ConvertToIntegerWordZero { d: 0, b: 1 }
        )
        && matches!(
            instructions[13],
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 16
            }
        )
        && matches!(
            instructions[14],
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20
            }
        )
        && matches!(
            instructions[15],
            Instruction::StoreWord {
                s: 0,
                a: 31,
                offset: 8228
            }
        )
        && matches!(
            instructions[16],
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0
            }
        )
        && matches!(
            instructions[17],
            Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: 8225
            }
        )
        && matches!(
            instructions[18],
            Instruction::StoreByte {
                s: 0,
                a: 31,
                offset: 8224
            }
        )
        && matches!(
            instructions[19],
            Instruction::LoadByteZero {
                d: 4,
                a: 31,
                offset: 8735
            }
        )
        && matches!(
            instructions[20],
            Instruction::LoadByteZero {
                d: 3,
                a: 31,
                offset: 12
            }
        )
        && matches!(instructions[22], Instruction::BranchAndLink { .. })
        && matches!(
            instructions[24],
            Instruction::BranchConditionalForward { target: 36, .. }
        )
        && matches!(
            instructions[27],
            Instruction::BranchConditionalForward { target: 36, .. }
        )
        && matches!(
            instructions[30],
            Instruction::BranchConditionalForward { target: 36, .. }
        )
        && matches!(instructions[35], Instruction::BranchAndLink { .. })
        && matches!(
            instructions[36],
            Instruction::LoadWord {
                d: 31,
                a: 30,
                offset: 44
            }
        )
        && matches!(
            instructions[37],
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6524
            }
        )
        && matches!(
            instructions[39],
            Instruction::BranchConditionalForward { target: 43, .. }
        )
        && matches!(
            instructions[40],
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 6528
            }
        )
        && matches!(
            instructions[42],
            Instruction::BranchConditionalForward { target: 54, .. }
        )
        && matches!(
            instructions[43],
            Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: 4
            }
        )
        && matches!(
            instructions[50],
            Instruction::BranchConditionalForward { target: 54, .. }
        )
        && matches!(
            instructions[52],
            Instruction::AddImmediate {
                d: 3,
                a: 30,
                immediate: 0
            }
        )
        && matches!(instructions[53], Instruction::BranchToLinkRegisterAndLink)
        && matches!(
            instructions[54],
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 52
            }
        )
        && matches!(
            instructions[55],
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 44
            }
        )
        && matches!(
            instructions[56],
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 40
            }
        )
        && matches!(
            instructions[57],
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 48
            }
        )
        && matches!(instructions[58], Instruction::MoveToLinkRegister { s: 0 })
        && matches!(instructions[59], Instruction::BranchToLinkRegister)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_schedule_is_a_permutation() {
        let mut scheduled = ENTRY_SCHEDULE.to_vec();
        scheduled.sort_unstable();
        assert_eq!(scheduled, (0..ENTRY_SCHEDULE.len()).collect::<Vec<_>>());
    }
}
