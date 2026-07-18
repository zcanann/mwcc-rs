//! Build 163's interleaved scheduler for distinct constant-store runs.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
    let mut loads = vec![0usize; count];
    let mut stores = vec![0usize; count];
    for (position, event) in events.iter().enumerate() {
        match *event {
            StoreEvent::Load(index) => loads[index] = position,
            StoreEvent::Store(index) => stores[index] = position,
        }
    }

    let colors: Vec<u8> = core::iter::once(GENERAL_SCRATCH).chain(3u8..=12).collect();
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
}
