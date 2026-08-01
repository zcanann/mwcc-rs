//! Build 163's interleaved scheduler for distinct constant-store runs.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Reschedule a physical run of serialized member constants using 2.3.3's
    /// two-value overlap window. The member base stays live throughout, so the
    /// interval colorer excludes it while reusing r0 as soon as each pending
    /// store releases the scratch value.
    pub(crate) fn schedule_legacy_member_constant_store_run(&mut self) {
        if self.behavior.constant_store_schedule_style
            != mwcc_versions::ConstantStoreScheduleStyle::InterleavedPairs
            || !self.output.entry_points.is_empty()
            || !self.output.jump_tables.is_empty()
            || !self.output.data_section_displacements.is_empty()
        {
            return;
        }
        let relocation_owners: Vec<usize> = self
            .output
            .relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect();
        if let Some(permutation) = schedule_serialized_member_constants(
            &mut self.output.instructions,
            &relocation_owners,
        ) {
            crate::remap_instruction_indices(self, &permutation);
        }
    }

    /// Emit a build-163 distinct-constant run.
    ///
    /// File-scope globals have no address-register dependency. The old scheduler
    /// therefore issues two constant materializations followed by the earliest
    /// pending store, repeating until every value is available and then draining
    /// the remaining stores. Its reverse greedy coloring gives r0 first, followed
    /// by r3..r12, and reuses a register when live intervals do not overlap.
    /// Pointer/member targets instead serialize through r0 so their address base
    /// remains undisturbed.
    pub(crate) fn emit_legacy_distinct_constant_store_run(
        &mut self,
        statements: &[Statement],
        assignments: &[(i32, u8)],
    ) -> Compilation<()> {
        let all_globals = statements.iter().all(|statement| {
            matches!(
                statement,
                Statement::Store {
                    target: Expression::Variable(_),
                    ..
                }
            )
        });
        if !all_globals {
            for (statement, &(constant, _)) in statements.iter().zip(assignments) {
                self.load_integer_constant(GENERAL_SCRATCH, constant as i64);
                self.prematerialized_constants = vec![(constant, GENERAL_SCRATCH)];
                self.emit_statement(statement)?;
            }
            self.prematerialized_constants.clear();
            return Ok(());
        }

        let events = interleaved_events(assignments.len());
        let registers = reverse_color(&events, assignments.len());
        self.prematerialized_constants = assignments
            .iter()
            .zip(&registers)
            .map(|(&(constant, _), &register)| (constant, register))
            .collect();
        for event in events {
            match event {
                StoreEvent::Load(index) => {
                    self.load_integer_constant(registers[index], assignments[index].0 as i64);
                }
                StoreEvent::Store(index) => self.emit_statement(&statements[index])?,
            }
        }
        self.prematerialized_constants.clear();
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum StoreEvent {
    Load(usize),
    Store(usize),
}

/// Issue two loads, then the oldest pending store; drain stores after the final
/// load. This is the instruction order observed for runs from two through seven
/// distinct values and generalizes without a sample-count table.
fn interleaved_events(count: usize) -> Vec<StoreEvent> {
    let mut events = Vec::with_capacity(count * 2);
    let mut next_store = 0usize;
    for first in (0..count).step_by(2) {
        events.push(StoreEvent::Load(first));
        if first + 1 < count {
            events.push(StoreEvent::Load(first + 1));
        }
        events.push(StoreEvent::Store(next_store));
        next_store += 1;
    }
    for index in next_store..count {
        events.push(StoreEvent::Store(index));
    }
    events
}

/// Reverse greedy interval coloring, using build 163's scratch-first register
/// order. Later values get the canonical r0/r3/r4... colors; earlier values
/// reuse those colors once their preceding store ends the live interval.
fn reverse_color(events: &[StoreEvent], count: usize) -> Vec<u8> {
    reverse_color_avoiding(events, count, &[])
}

fn reverse_color_avoiding(events: &[StoreEvent], count: usize, forbidden: &[u8]) -> Vec<u8> {
    let mut loads = vec![0usize; count];
    let mut stores = vec![0usize; count];
    for (position, event) in events.iter().enumerate() {
        match *event {
            StoreEvent::Load(index) => loads[index] = position,
            StoreEvent::Store(index) => stores[index] = position,
        }
    }

    let colors: Vec<u8> = core::iter::once(GENERAL_SCRATCH)
        .chain(3u8..=12)
        .filter(|register| !forbidden.contains(register))
        .collect();
    let mut registers = vec![GENERAL_SCRATCH; count];
    for index in (0..count).rev() {
        registers[index] = colors
            .iter()
            .copied()
            .find(|candidate| {
                ((index + 1)..count).all(|later| {
                    let overlaps = loads[later] < stores[index] && loads[index] < stores[later];
                    !overlaps || registers[later] != *candidate
                })
            })
            .expect("constant-store run exceeds the planned register set");
    }
    registers
}

/// A member base consumes one of the old scheduler's initial issue lanes. It
/// therefore commits the first value immediately, then maintains a two-value
/// load/store window for the rest of the run.
fn member_interleaved_events(unique_count: usize, store_count: usize) -> Vec<StoreEvent> {
    let mut events = Vec::with_capacity(unique_count + store_count);
    if unique_count == 0 || store_count == 0 {
        return events;
    }
    events.extend([StoreEvent::Load(0), StoreEvent::Store(0)]);
    let mut next_store = 1;
    for first in (1..unique_count).step_by(2) {
        events.push(StoreEvent::Load(first));
        if first + 1 < unique_count {
            events.push(StoreEvent::Load(first + 1));
        }
        if next_store < store_count {
            events.push(StoreEvent::Store(next_store));
            next_store += 1;
        }
    }
    for index in next_store..store_count {
        events.push(StoreEvent::Store(index));
    }
    events
}

/// Color each unique constant from its materialization through its last store.
/// Store events name source-order stores, so repeated constants extend one
/// interval rather than introducing redundant materializations.
fn color_member_constants(
    events: &[StoreEvent],
    value_for_store: &[usize],
    unique_count: usize,
    forbidden: &[u8],
) -> Vec<u8> {
    let mut loads = vec![0usize; unique_count];
    let mut last_stores = vec![0usize; unique_count];
    for (position, event) in events.iter().enumerate() {
        match *event {
            StoreEvent::Load(value) => loads[value] = position,
            StoreEvent::Store(store) => last_stores[value_for_store[store]] = position,
        }
    }

    let colors: Vec<u8> = core::iter::once(GENERAL_SCRATCH)
        .chain(3u8..=12)
        .filter(|register| !forbidden.contains(register))
        .collect();
    let mut registers = vec![GENERAL_SCRATCH; unique_count];
    for value in (0..unique_count).rev() {
        registers[value] = colors
            .iter()
            .copied()
            .find(|candidate| {
                ((value + 1)..unique_count).all(|later| {
                    let overlaps = loads[later] < last_stores[value]
                        && loads[value] < last_stores[later];
                    !overlaps || registers[later] != *candidate
                })
            })
            .expect("constant-store run exceeds the planned register set");
    }
    registers
}

fn serialized_member_pair(instructions: &[Instruction], at: usize) -> Option<(i16, u8)> {
    let Instruction::AddImmediate {
        d: GENERAL_SCRATCH,
        a: 0,
        immediate,
    } = *instructions.get(at)?
    else {
        return None;
    };
    let (source, base) = store_source_and_base(instructions.get(at + 1)?)?;
    (source == GENERAL_SCRATCH && base > 2).then_some((immediate, base))
}

fn store_source_and_base(instruction: &Instruction) -> Option<(u8, u8)> {
    match *instruction {
        Instruction::StoreWord { s, a, .. }
        | Instruction::StoreByte { s, a, .. }
        | Instruction::StoreHalfword { s, a, .. } => Some((s, a)),
        _ => None,
    }
}

fn set_store_source(instruction: &mut Instruction, source: u8) {
    match instruction {
        Instruction::StoreWord { s, .. }
        | Instruction::StoreByte { s, .. }
        | Instruction::StoreHalfword { s, .. } => *s = source,
        _ => unreachable!("serialized member store changed kind"),
    }
}

fn schedule_serialized_member_constants(
    instructions: &mut Vec<Instruction>,
    relocation_owners: &[usize],
) -> Option<Vec<usize>> {
    let old_len = instructions.len();
    for start in 0..instructions.len() {
        let Some((_, base)) = serialized_member_pair(instructions, start) else {
            continue;
        };
        let mut constants = Vec::new();
        let mut stores = Vec::new();
        let mut at = start;
        while let Some((constant, candidate_base)) = serialized_member_pair(instructions, at) {
            if candidate_base != base {
                break;
            }
            constants.push(constant);
            stores.push(instructions[at + 1].clone());
            at += 2;
        }
        if constants.len() < 3
            || relocation_owners.iter().any(|owner| (start..at).contains(owner))
        {
            continue;
        }
        let has_incoming_branch = instructions.iter().any(|instruction| {
            matches!(instruction,
                Instruction::Branch { target }
                    | Instruction::BranchConditionalForward { target, .. }
                    if (start..at).contains(target))
        });
        if has_incoming_branch {
            continue;
        }

        let mut unique_constants = Vec::new();
        let mut value_for_store = Vec::with_capacity(constants.len());
        for &constant in &constants {
            let value = match unique_constants.iter().position(|value| *value == constant) {
                Some(value) => value,
                None => {
                    unique_constants.push(constant);
                    unique_constants.len() - 1
                }
            };
            value_for_store.push(value);
        }
        let events = member_interleaved_events(unique_constants.len(), stores.len());
        let registers = color_member_constants(
            &events,
            &value_for_store,
            unique_constants.len(),
            &[base],
        );
        let mut replacement = Vec::with_capacity(unique_constants.len() + stores.len());
        for event in events {
            match event {
                StoreEvent::Load(index) => replacement.push(Instruction::AddImmediate {
                    d: registers[index],
                    a: 0,
                    immediate: unique_constants[index],
                }),
                StoreEvent::Store(index) => {
                    let mut store = stores[index].clone();
                    set_store_source(&mut store, registers[value_for_store[index]]);
                    replacement.push(store);
                }
            }
        }
        let new_range_len = replacement.len();
        instructions.splice(start..at, replacement);
        let removed = (at - start) - new_range_len;
        let permutation = (0..old_len)
            .map(|old| {
                if old < start {
                    old
                } else if old < at {
                    start
                } else {
                    old - removed
                }
            })
            .collect();
        return Some(permutation);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_coloring_matches_observed_two_through_seven_value_runs() {
        let expected = [
            vec![3, 0],
            vec![0, 3, 0],
            vec![0, 4, 3, 0],
            vec![3, 0, 4, 3, 0],
            vec![3, 0, 5, 4, 3, 0],
            vec![0, 3, 0, 5, 4, 3, 0],
        ];
        for (count, expected) in (2usize..=7).zip(expected) {
            let events = interleaved_events(count);
            assert_eq!(reverse_color(&events, count), expected);
        }
    }

    #[test]
    fn member_window_reuses_scratch_without_clobbering_its_base() {
        let events = member_interleaved_events(4, 4);
        assert_eq!(color_member_constants(&events, &[0, 1, 2, 3], 4, &[3]), [0, 0, 4, 0]);

        let mut instructions = Vec::new();
        for (constant, offset) in [(0, 0), (8, 1), (1, 2), (10, 3)] {
            instructions.push(Instruction::load_immediate(0, constant));
            instructions.push(Instruction::StoreByte {
                s: 0,
                a: 3,
                offset,
            });
        }
        assert!(schedule_serialized_member_constants(&mut instructions, &[]).is_some());
        assert!(matches!(instructions.as_slice(), [
            Instruction::AddImmediate { d: 0, immediate: 0, .. },
            Instruction::StoreByte { s: 0, offset: 0, .. },
            Instruction::AddImmediate { d: 0, immediate: 8, .. },
            Instruction::AddImmediate { d: 4, immediate: 1, .. },
            Instruction::StoreByte { s: 0, offset: 1, .. },
            Instruction::AddImmediate { d: 0, immediate: 10, .. },
            Instruction::StoreByte { s: 4, offset: 2, .. },
            Instruction::StoreByte { s: 0, offset: 3, .. },
        ]));
    }

    #[test]
    fn member_window_reuses_mixed_repeated_constants() {
        let constants = [
            122, 0, 79, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 3, 0, 0, 0, 0,
            0, 0, 0, 3, 0, 0, 0, 0, 128,
        ];
        let mut instructions = Vec::new();
        for (offset, constant) in constants.into_iter().enumerate() {
            instructions.push(Instruction::load_immediate(0, constant));
            instructions.push(Instruction::StoreByte {
                s: 0,
                a: 3,
                offset: offset as i16,
            });
        }

        let permutation = schedule_serialized_member_constants(&mut instructions, &[]).unwrap();
        assert_eq!(permutation.len(), 64);
        assert_eq!(instructions.len(), 39);
        assert!(matches!(instructions.as_slice(), [
            Instruction::AddImmediate { d: 0, immediate: 122, .. },
            Instruction::StoreByte { s: 0, offset: 0, .. },
            Instruction::AddImmediate { d: 7, immediate: 0, .. },
            Instruction::AddImmediate { d: 0, immediate: 79, .. },
            Instruction::StoreByte { s: 7, offset: 1, .. },
            Instruction::AddImmediate { d: 6, immediate: 7, .. },
            Instruction::AddImmediate { d: 5, immediate: 1, .. },
            Instruction::StoreByte { s: 0, offset: 2, .. },
            Instruction::AddImmediate { d: 4, immediate: 3, .. },
            Instruction::AddImmediate { d: 0, immediate: 128, .. },
            Instruction::StoreByte { s: 6, offset: 3, .. },
            ..,
            Instruction::StoreByte { s: 0, offset: 31, .. },
        ]));
    }
}
