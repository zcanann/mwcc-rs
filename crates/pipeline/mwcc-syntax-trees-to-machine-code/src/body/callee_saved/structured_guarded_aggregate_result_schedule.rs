//! Linkage-first scheduling for a guarded aggregate-result member comparison.
//!
//! Build 163 loads a member-call receiver before materializing the hidden
//! aggregate return address. After the call it loads the compared result word
//! first, then the comparison peer. This keeps the aggregate in the ordinary
//! result register and places the peer in scratch.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_guarded_aggregate_result_compare(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let capture = std::env::var_os("MWCC_CAPTURE_FUNCTION")
            .is_some_and(|name| name == std::ffi::OsStr::new(&self.output.name));
        if capture {
            eprintln!(
                "guarded aggregate slots: {:?}",
                self.frame_slots
                    .iter()
                    .map(|(name, slot)| (name, slot.offset, slot.value_type))
                    .collect::<Vec<_>>()
            );
        }
        let Some(plan) = guarded_aggregate_result_plan(
            &self.output.instructions,
            self.frame_slots.values().filter_map(|slot| {
                matches!(slot.value_type, Type::Struct { .. }).then_some(slot.offset)
            }),
        ) else {
            if capture {
                eprintln!(
                    "guarded aggregate schedule declined: {:?}",
                    self.output.instructions
                );
            }
            return;
        };

        self.move_instruction_before(plan.start + 1, plan.start);
        self.output.instructions[plan.start + 3] = Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: plan.frame_offset,
        };
        self.output.instructions[plan.start + 4] = Instruction::LoadWord {
            d: 0,
            a: plan.peer_base,
            offset: plan.peer_offset,
        };
        self.output.instructions[plan.start + 5] = Instruction::CompareLogicalWord { a: 0, b: 3 };
    }
}

#[derive(Clone, Copy)]
struct GuardedAggregateResultPlan {
    start: usize,
    frame_offset: i16,
    peer_base: u8,
    peer_offset: i16,
}

fn guarded_aggregate_result_plan(
    instructions: &[Instruction],
    frame_offsets: impl Iterator<Item = i16>,
) -> Option<GuardedAggregateResultPlan> {
    for frame_offset in frame_offsets {
        let Some(start) = instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: 3,
                        a: 1,
                        immediate,
                    },
                    Instruction::LoadWord { d: 4, .. },
                    Instruction::BranchAndLink { .. },
                    Instruction::LoadWord { d: 3, a, .. },
                    Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset,
                    },
                    Instruction::CompareLogicalWord { a: 3, b: 0 },
                ] if *immediate == frame_offset && *offset == frame_offset && *a != 1
            )
        }) else {
            continue;
        };
        let Instruction::LoadWord {
            a: peer_base,
            offset: peer_offset,
            ..
        } = instructions[start + 3]
        else {
            unreachable!("guarded aggregate peer load was recognized")
        };
        return Some(GuardedAggregateResultPlan {
            start,
            frame_offset,
            peer_base,
            peer_offset,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_hidden_result_and_peer_load_order() {
        let instructions = [
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: 392,
            },
            Instruction::BranchAndLink {
                target: "getID".into(),
            },
            Instruction::LoadWord {
                d: 3,
                a: 30,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 24,
            },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ];

        let plan = guarded_aggregate_result_plan(&instructions, [24].into_iter())
            .expect("the aggregate-result member comparison should be recognized");
        assert_eq!(plan.start, 0);
        assert_eq!(plan.frame_offset, 24);
        assert_eq!(plan.peer_base, 30);
        assert_eq!(plan.peer_offset, 8);
    }
}
