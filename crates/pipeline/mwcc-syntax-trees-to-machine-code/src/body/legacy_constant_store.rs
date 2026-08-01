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
        {
            return;
        }
        let relocation_owners: Vec<usize> = self
            .output
            .relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect();
        schedule_serialized_member_constants(
            &mut self.output.instructions,
            &relocation_owners,
        );
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
fn member_interleaved_events(count: usize) -> Vec<StoreEvent> {
    let mut events = Vec::with_capacity(count * 2);
    if count == 0 {
        return events;
    }
    events.extend([StoreEvent::Load(0), StoreEvent::Store(0)]);
    let mut next_store = 1;
    for first in (1..count).step_by(2) {
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
) -> bool {
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
        let mut distinct = constants.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != constants.len() {
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
        let events = member_interleaved_events(constants.len());
        let registers = reverse_color_avoiding(&events, constants.len(), &[base]);
        let mut replacement = Vec::with_capacity(constants.len() * 2);
        for event in events {
            match event {
                StoreEvent::Load(index) => replacement.push(Instruction::AddImmediate {
                    d: registers[index],
                    a: 0,
                    immediate: constants[index],
                }),
                StoreEvent::Store(index) => {
                    let mut store = stores[index].clone();
                    set_store_source(&mut store, registers[index]);
                    replacement.push(store);
                }
            }
        }
        instructions.splice(start..at, replacement);
        return true;
    }
    false
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
        let events = member_interleaved_events(4);
        assert_eq!(reverse_color_avoiding(&events, 4, &[3]), [0, 0, 4, 0]);

        let mut instructions = Vec::new();
        for (constant, offset) in [(0, 0), (8, 1), (1, 2), (10, 3)] {
            instructions.push(Instruction::load_immediate(0, constant));
            instructions.push(Instruction::StoreByte {
                s: 0,
                a: 3,
                offset,
            });
        }
        assert!(schedule_serialized_member_constants(&mut instructions, &[]));
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
}
