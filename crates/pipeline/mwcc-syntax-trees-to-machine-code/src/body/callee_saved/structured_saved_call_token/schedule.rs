//! Profile-specific issue order after saved-token role allocation.
use super::*;

impl Generator {
    pub(crate) fn schedule_saved_call_token(&mut self, function: &Function) {
        use mwcc_versions::{Optimization, OptimizationGoal, SavedCallTokenStyle};
        let style = self.behavior.saved_call_token_style;
        if style == SavedCallTokenStyle::Structured
            || self.behavior.optimization != Optimization::O4
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.behavior.power_pc_7400_scheduling_enabled()
            || function.peephole_disabled
            || retained_token(function).is_none()
            || self.frame_size != 32
            || !self.frame_slots.is_empty()
            || self.callee_saved.len() != 3
            || !scalar_token_frame(&self.output.instructions)
        {
            return;
        }
        use Instruction::*;
        let Some(first_call) = self
            .output
            .instructions
            .iter()
            .position(|i| matches!(i, BranchAndLink { .. }))
        else {
            return;
        };
        // This packet proves the layout selected above survived physical allocation.
        if first_call < 5
            || !matches!(
                &self.output.instructions[first_call - 5..first_call],
                [
                    StoreWord {
                        s: 31,
                        a: 1,
                        offset: 28
                    },
                    StoreWord {
                        s: 30,
                        a: 1,
                        offset: 24
                    },
                    AddImmediate {
                        d: 30,
                        a: 4,
                        immediate: 0
                    },
                    StoreWord {
                        s: 29,
                        a: 1,
                        offset: 20
                    },
                    AddImmediate {
                        d: 29,
                        a: 3,
                        immediate: 0
                    },
                ]
            )
        {
            return;
        }
        let Some(last_call) = self
            .output
            .instructions
            .iter()
            .rposition(|i| matches!(i, BranchAndLink { .. }))
        else {
            return;
        };
        let Some(epilogue_order) =
            token_epilogue_order(&self.output.instructions[last_call + 1..], style)
        else {
            return;
        };
        let patched = style == SavedCallTokenStyle::LegacyPatched;
        let start = first_call + 1;
        let init_packet = self
            .output
            .instructions
            .get(start..start + 9)
            .is_some_and(|packet| {
                matches!(
                    packet,
                    [
                        AddImmediate {
                            d: 31,
                            a: 3,
                            immediate: 0
                        },
                        AddImmediate { d: 0, a: 0, .. },
                        StoreWord { s: 0, a: 0, .. },
                        LoadWord { d: 0, a: 0, .. },
                        StoreWord { s: 0, a: 29, .. },
                        StoreWord { s: 30, a: 0, .. },
                        AddImmediateShifted { d: 3, a: 0, .. },
                        AddImmediate { d: 3, a: 3, .. },
                        BranchAndLink { .. },
                    ]
                )
            });
        if init_packet {
            if let AddImmediateShifted { d, .. } = &mut self.output.instructions[start + 6] {
                *d = 4;
            }
            if let AddImmediate { a, .. } = &mut self.output.instructions[start + 7] {
                *a = 4;
            }
            self.order_token_packet(
                start,
                if patched {
                    &[1, 0, 2, 6, 7, 3, 4, 5]
                } else {
                    &[1, 2, 6, 0, 3, 7, 4, 5]
                },
            );
        } else if matches!(
            self.output.instructions.get(start..start + 2),
            Some([
                AddImmediate {
                    d: 31,
                    a: 3,
                    immediate: 0
                },
                LoadWord { d: 0, a: 0, .. },
            ])
        ) {
            self.order_token_packet(start, &[1, 0]);
        }
        // Fill the address-add latency with independent rounded-length and pointer arguments.
        for index in first_call + 1..self.output.instructions.len().saturating_sub(5) {
            if matches!(
                &self.output.instructions[index..index + 6],
                [
                    AddImmediateShifted { d: 3, a: 3, .. },
                    AddImmediate { d: 3, a: 3, .. },
                    Or { a: 4, s: 29, b: 29 },
                    AddImmediate { d: 0, a: 30, .. },
                    AndContiguousMask { a: 5, s: 0, .. },
                    BranchAndLink { .. },
                ]
            ) {
                self.output.instructions[index + 2] = AddImmediate {
                    d: 4,
                    a: 29,
                    immediate: 0,
                };
                self.order_token_packet(
                    index,
                    if patched {
                        &[0, 3, 2, 1, 4]
                    } else {
                        &[0, 3, 2, 4, 1]
                    },
                );
            }
        }
        let Some(last_call) = self
            .output
            .instructions
            .iter()
            .rposition(|i| matches!(i, BranchAndLink { .. }))
        else {
            return;
        };
        if last_call >= 2
            && matches!(
                &self.output.instructions[last_call - 2..last_call],
                [StoreByte { s: 0, a: 0, .. }, Or { a: 3, s: 31, b: 31 },]
            )
        {
            self.order_token_packet(last_call - 2, &[1, 0]);
        }
        self.order_token_packet(last_call + 1, &epilogue_order);
        if patched {
            for instruction in &mut self.output.instructions {
                match instruction {
                    StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: at,
                    } if *at == -32 => *at = -24,
                    AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: at,
                    } if *at == 32 => *at = 24,
                    StoreWord {
                        s: 29..=31,
                        a: 1,
                        offset: at,
                    }
                    | LoadWord {
                        d: 29..=31,
                        a: 1,
                        offset: at,
                    } if matches!(*at, 20 | 24 | 28) => *at -= 8,
                    _ => {}
                }
            }
            self.frame_size = 24;
        }
    }

    fn order_token_packet(&mut self, start: usize, order: &[usize]) {
        let mut positions: Vec<_> = (0..order.len()).collect();
        for (to, wanted) in order.iter().enumerate() {
            let from = positions
                .iter()
                .position(|index| index == wanted)
                .expect("packet permutation");
            if from != to {
                crate::move_instruction_before_retargeting(self, start + from, start + to);
                let moved = positions.remove(from);
                positions.insert(to, moved);
            }
        }
    }
}

fn token_epilogue_order(
    tail: &[Instruction],
    style: mwcc_versions::SavedCallTokenStyle,
) -> Option<Vec<usize>> {
    use mwcc_versions::SavedCallTokenStyle;
    use Instruction::*;
    let mut roles = [None; 8];
    for (index, instruction) in tail.iter().enumerate() {
        let role = match instruction {
            LoadWord { d: 0, a: 1, offset }
                if *offset
                    == if style == SavedCallTokenStyle::LegacyPatched {
                        4
                    } else {
                        36
                    } =>
            {
                0
            }
            AddImmediate { d: 3, a: 0, .. } => 1,
            LoadWord {
                d: 31,
                a: 1,
                offset: 28,
            } => 2,
            LoadWord {
                d: 30,
                a: 1,
                offset: 24,
            } => 3,
            LoadWord {
                d: 29,
                a: 1,
                offset: 20,
            } => 4,
            AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            } => 5,
            MoveToLinkRegister { s: 0 } => 6,
            BranchToLinkRegister => 7,
            _ => return None,
        };
        if roles[role].replace(index).is_some() {
            return None;
        }
    }
    if roles
        .iter()
        .enumerate()
        .any(|(role, index)| role != 1 && index.is_none())
    {
        return None;
    }
    let order = match style {
        SavedCallTokenStyle::LegacyInterleaved => [0, 1, 2, 3, 6, 4, 5, 7],
        SavedCallTokenStyle::LegacyStackLast => [0, 1, 2, 3, 4, 5, 6, 7],
        SavedCallTokenStyle::LegacyPatched => [2, 1, 3, 4, 5, 0, 6, 7],
        SavedCallTokenStyle::Structured => return None,
    };
    let permutation: Vec<_> = order.into_iter().filter_map(|role| roles[role]).collect();
    Some(permutation)
}

/// The compact patched frame must not overlap outgoing arguments or scratch storage.
fn scalar_token_frame(instructions: &[Instruction]) -> bool {
    use Instruction::*;
    instructions.iter().all(|instruction| {
        let uses_stack = mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| operand.class == mwcc_vreg::Class::General && operand.register == 1);
        !uses_stack
            || matches!(
                instruction,
                StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32
                } | AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 32
                } | StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4
                } | LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4 | 36
                } | StoreWord {
                    s: 29..=31,
                    a: 1,
                    offset: 20 | 24 | 28
                } | LoadWord {
                    d: 29..=31,
                    a: 1,
                    offset: 20 | 24 | 28
                }
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_versions::SavedCallTokenStyle;
    use Instruction::*;

    #[test]
    fn compact_token_frame_rejects_outgoing_arguments_and_scratch_uses() {
        assert!(scalar_token_frame(&[
            StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32
            },
            StoreWord {
                s: 31,
                a: 1,
                offset: 28
            },
            LoadWord {
                d: 31,
                a: 1,
                offset: 28
            }
        ]));
        for instruction in [
            StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            LoadWord {
                d: 0,
                a: 1,
                offset: 16,
            },
            StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            },
        ] {
            assert!(!scalar_token_frame(&[instruction]));
        }
    }

    #[test]
    fn token_epilogues_require_the_profile_linkage_base_and_complete_restores() {
        let mut tail = vec![
            LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            LoadWord {
                d: 31,
                a: 1,
                offset: 28,
            },
            LoadWord {
                d: 30,
                a: 1,
                offset: 24,
            },
            LoadWord {
                d: 29,
                a: 1,
                offset: 20,
            },
            AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            MoveToLinkRegister { s: 0 },
            BranchToLinkRegister,
        ];
        assert!(token_epilogue_order(&tail, SavedCallTokenStyle::LegacyInterleaved).is_some());
        assert!(token_epilogue_order(&tail, SavedCallTokenStyle::LegacyPatched).is_none());
        tail[0] = LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        };
        assert!(token_epilogue_order(&tail, SavedCallTokenStyle::LegacyPatched).is_some());
        tail[3] = LoadWord {
            d: 30,
            a: 1,
            offset: 24,
        };
        assert!(token_epilogue_order(&tail, SavedCallTokenStyle::LegacyPatched).is_none());
    }
}
