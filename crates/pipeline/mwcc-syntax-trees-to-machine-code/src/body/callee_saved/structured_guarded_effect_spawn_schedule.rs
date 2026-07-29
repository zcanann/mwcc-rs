//! Scheduling for a guarded effect transaction selected by a mutating boolean.
//!
//! Build 163 preserves the inlined boolean helper's canonical 0/1 diamond,
//! keeps the attributes in incoming r4 until the selected bone lookup, and
//! lowers the following two-case kind switch as a range-shaped dispatch. The
//! two effect arms share one packet schedule but retain independent constants.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_guarded_effect_spawn(&mut self, function: &Function) {
        if !function.locals.iter().any(|local| {
            local.array_length.is_some()
                && !super::structured_locals::body_uses_local(&function.statements, &local.name)
        }) {
            return;
        }
        let Some(plan) = guarded_effect_spawn_plan(&self.output.instructions) else {
            return;
        };

        schedule_spawn_arm(self, plan.switch_call + 17, plan.entry);
        schedule_spawn_arm(self, plan.switch_call + 3, plan.entry);
        lower_range_switch(self, plan.switch_call);

        schedule_first_bone_lookup(self, plan.first_bone, plan.receiver);
        schedule_second_bone_lookup(self, plan.second_bone);
        self.remove_structured_condition_instruction(plan.second_motion_load);

        let frame_offset = self.frame_size.saturating_sub(20);
        relocate_effect_frame_value(
            &mut self.output.instructions,
            plan.frame_offset,
            frame_offset,
        );
        for slot in self.frame_slots.values_mut() {
            if slot.offset == plan.frame_offset {
                slot.offset = frame_offset;
            }
        }

        schedule_mutating_boolean_entry(self, plan);
        // This transaction's four six-label optimizer groups are visible to
        // its own pool and unwind objects, then MWCC restores the enclosing
        // translation-unit ordinal counter.
        self.output.post_function_counter_rollback = 4 * 6;
        self.output.deferred_next_constant_scope_bump = 4 * 6;
    }
}

fn schedule_spawn_arm(generator: &mut Generator, start: usize, entry: u8) {
    // parts; index; object; entry; joint; member; crclr; frame; constants; call
    generator.move_instruction_before(start + 5, start);
    generator.move_instruction_before(start + 6, start + 1);
    generator.move_instruction_before(start + 3, start + 2);
    generator.move_instruction_before(start + 7, start + 4);
    generator.move_instruction_before(start + 9, start + 6);
    generator.move_instruction_before(start + 9, start + 7);

    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[start] else {
        unreachable!("spawn parts load changed after recognition");
    };
    *d = 4;
    let Instruction::LoadWord { d, a, .. } = &mut generator.output.instructions[start + 2] else {
        unreachable!("spawn receiver load changed after recognition");
    };
    *d = 5;
    *a = entry;
    let Instruction::LoadWordIndexed { a, .. } = &mut generator.output.instructions[start + 4]
    else {
        unreachable!("spawn indexed load changed after recognition");
    };
    *a = 4;
    let Instruction::AddImmediate { a, .. } = &mut generator.output.instructions[start + 5] else {
        unreachable!("spawn member address changed after recognition");
    };
    *a = 5;
}

fn lower_range_switch(generator: &mut Generator, call: usize) {
    generator.move_instruction_before(call + 15, call + 1);
    generator.move_instruction_before(call + 16, call + 2);
    crate::insert_instruction_retargeting(
        generator,
        call + 3,
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 0,
        },
    );
    crate::insert_instruction_retargeting(generator, call + 6, Instruction::Branch { target: 0 });

    let first_case = call + 7;
    let second_case = call + 19;
    let default = call + 30;
    generator.output.instructions[call + 2] = Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: second_case,
    };
    if let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[call + 3]
    {
        *target = default;
    }
    generator.output.instructions[call + 5] = Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: first_case,
    };
    generator.output.instructions[call + 6] = Instruction::Branch { target: default };
    generator.output.instructions[call + 18] = Instruction::Branch { target: default };
}

fn schedule_first_bone_lookup(generator: &mut Generator, call: usize, receiver: u8) {
    let start = call - 4;
    generator.move_instruction_before(start + 2, start + 1);
    generator.move_instruction_before(start + 3, start + 2);
    generator.output.instructions[start + 1] = Instruction::AddImmediate {
        d: 3,
        a: receiver,
        immediate: 0,
    };
    let (Instruction::Or { a, s: 3, b: 3 } | Instruction::AddImmediate { d: a, a: 3, .. }) =
        generator.output.instructions[call + 1]
    else {
        unreachable!("first bone result changed after recognition");
    };
    generator.output.instructions[call + 1] = Instruction::Or { a, s: 3, b: 3 };
}

fn schedule_second_bone_lookup(generator: &mut Generator, call: usize) {
    let start = call - 6;
    generator.move_instruction_before(start + 4, start + 1);
    generator.move_instruction_before(start + 5, start + 3);
    let Instruction::LoadFloatSingle { a, .. } = &mut generator.output.instructions[start + 2]
    else {
        unreachable!("second bone attribute load changed after recognition");
    };
    *a = 4;
    let Instruction::FloatMultiplySingle { d, .. } = &mut generator.output.instructions[start + 4]
    else {
        unreachable!("second bone multiply changed after recognition");
    };
    *d = 0;
    let Instruction::StoreFloatSingle { s, .. } = &mut generator.output.instructions[start + 5]
    else {
        unreachable!("second bone frame store changed after recognition");
    };
    *s = 0;
    let (Instruction::Or { a, s: 3, b: 3 } | Instruction::AddImmediate { d: a, a: 3, .. }) =
        generator.output.instructions[call + 1]
    else {
        unreachable!("second bone result changed after recognition");
    };
    generator.output.instructions[call + 1] = Instruction::Or { a, s: 3, b: 3 };
}

fn relocate_effect_frame_value(instructions: &mut [Instruction], old: i16, new: i16) {
    for instruction in instructions {
        match instruction {
            Instruction::StoreFloatSingle { a: 1, offset, .. } if *offset == old => {
                *offset = new;
            }
            Instruction::AddImmediate {
                d: 8,
                a: 1,
                immediate,
            } if *immediate == old => {
                *immediate = new;
            }
            _ => {}
        }
    }
}

fn schedule_mutating_boolean_entry(generator: &mut Generator, plan: GuardedEffectSpawnPlan) {
    let prefix = plan.entry_packet - 5;
    generator.move_instruction_before(prefix + 2, prefix + 1);
    generator.move_instruction_before(prefix + 3, prefix + 2);
    generator.move_instruction_before(prefix + 4, prefix + 3);
    generator.move_instruction_before(prefix + 6, prefix + 5);

    let entry_packet = plan.entry_packet;
    generator.remove_structured_condition_instruction(entry_packet + 4);
    let Instruction::LoadByteZero { d, .. } = &mut generator.output.instructions[entry_packet]
    else {
        unreachable!("entry flag load changed after recognition");
    };
    *d = 3;
    let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[entry_packet + 1]
    else {
        unreachable!("entry attribute load changed after recognition");
    };
    *d = 4;
    generator.output.instructions[entry_packet + 2] = Instruction::RotateAndMaskRecord {
        a: 0,
        s: 3,
        shift: 26,
        begin: 31,
        end: 31,
    };
    generator.output.instructions[entry_packet + 4] = Instruction::load_immediate(0, 0);
    let Instruction::RotateAndMaskInsert { a, s, .. } =
        &mut generator.output.instructions[entry_packet + 5]
    else {
        unreachable!("entry flag clear changed after recognition");
    };
    *a = 3;
    *s = 0;
    let Instruction::StoreByte { s, .. } = &mut generator.output.instructions[entry_packet + 6]
    else {
        unreachable!("entry flag store changed after recognition");
    };
    *s = 3;

    let insertion = entry_packet + 7;
    for (offset, instruction) in [
        Instruction::load_immediate(0, 1),
        Instruction::Branch { target: 0 },
        Instruction::load_immediate(0, 0),
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 0,
        },
    ]
    .into_iter()
    .enumerate()
    {
        crate::insert_instruction_retargeting(generator, insertion + offset, instruction);
    }
    generator.output.instructions[entry_packet + 3] = Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: entry_packet + 9,
    };
    generator.output.instructions[entry_packet + 8] = Instruction::Branch {
        target: entry_packet + 10,
    };
    let epilogue = generator.output.instructions.len();
    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[entry_packet + 11]
    else {
        unreachable!("canonical boolean guard changed after insertion");
    };
    *target = epilogue;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GuardedEffectSpawnPlan {
    entry_packet: usize,
    entry: u8,
    receiver: u8,
    first_bone: usize,
    second_motion_load: usize,
    second_bone: usize,
    switch_call: usize,
    frame_offset: i16,
}

fn guarded_effect_spawn_plan(instructions: &[Instruction]) -> Option<GuardedEffectSpawnPlan> {
    instructions
        .windows(61)
        .enumerate()
        .find_map(|(start, window)| {
            if start < 5 {
                return None;
            }
            let [
                Instruction::LoadWord {
                    d: _attributes,
                    a: receiver,
                    ..
                },
                Instruction::LoadByteZero {
                    a: flag_receiver,
                    offset: flag_offset,
                    ..
                },
                Instruction::RotateAndMaskRecord { .. },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadByteZero {
                    a: reloaded_receiver,
                    offset: reloaded_offset,
                    ..
                },
                Instruction::AddImmediate {
                    a: 0, immediate: 0, ..
                },
                Instruction::RotateAndMaskInsert { .. },
                Instruction::StoreByte {
                    a: stored_receiver,
                    offset: stored_offset,
                    ..
                },
                ..,
            ] = window
            else {
                return None;
            };
            let first_bone = start + 18;
            let second_motion_load = start + 21;
            let second_bone = start + 30;
            let switch_call = start + 33;
            let (
                Instruction::BranchAndLink {
                    target: first_target,
                },
                Instruction::BranchAndLink {
                    target: second_target,
                },
                Instruction::BranchAndLink { .. },
                Instruction::BranchAndLink {
                    target: first_spawn,
                },
                Instruction::BranchAndLink {
                    target: second_spawn,
                },
            ) = (
                &instructions[first_bone],
                &instructions[second_bone],
                &instructions[switch_call],
                &instructions[start + 46],
                &instructions[start + 60],
            )
            else {
                return None;
            };
            let Instruction::StoreFloatSingle {
                a: 1,
                offset: frame_offset,
                ..
            } = instructions[start + 15]
            else {
                return None;
            };
            if receiver != flag_receiver
                || receiver != reloaded_receiver
                || receiver != stored_receiver
                || flag_offset != reloaded_offset
                || flag_offset != stored_offset
                || first_target != second_target
                || first_spawn != second_spawn
                || !matches!(
                    instructions[second_motion_load],
                    Instruction::LoadWord {
                        d: 0,
                        a,
                        offset: 16
                    } if a == *receiver
                )
                || !matches!(
                    instructions[start + 27],
                    Instruction::StoreFloatSingle {
                        a: 1,
                        offset,
                        ..
                    } if offset == frame_offset
                )
                || !matches!(
                    instructions.get(start - 2),
                    Some(Instruction::Or { a: entry, s: 3, b: 3 })
                        if *entry >= mwcc_vreg::VIRTUAL_BASE
                )
            {
                return None;
            }
            let entry = match instructions[start - 2] {
                Instruction::Or { a, .. } => a,
                _ => unreachable!("entry copy was validated above"),
            };
            Some(GuardedEffectSpawnPlan {
                entry_packet: start,
                entry,
                receiver: *receiver,
                first_bone,
                second_motion_load,
                second_bone,
                switch_call,
                frame_offset,
            })
        })
}
