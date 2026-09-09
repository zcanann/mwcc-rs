//! Share short-lived cursor setup addresses after the BSS layout is known.
//!
//! Source lowering supplies groups; this pass proves their selected address
//! packet and a free physical temporary. It neither extends a base across a
//! call nor changes the saved-register frame. A separate wide-address recipe
//! consumes version policy without duplicating packet validation or rewriting.

mod wide;

use std::collections::{HashMap, HashSet};

use mwcc_machine_code::{
    DeferredDisplacement, DeferredDisplacementTarget, Instruction, MachineFunction, Relocation,
    RelocationKind, RelocationTarget,
};

pub(super) fn share(function: &mut MachineFunction, offsets: &HashMap<String, u32>) {
    let groups = std::mem::take(&mut function.temporary_bss_address_groups);
    if groups.is_empty()
        || function.is_asm
        || !function.jump_tables.is_empty()
        || function.instructions.iter().any(|i| {
            matches!(
                i,
                Instruction::VerbatimWord { .. } | Instruction::BranchToCountRegister
            )
        })
    {
        return;
    }
    let mut seen = HashSet::new();
    for symbols in groups {
        if symbols.len() < 3
            || symbols.iter().collect::<HashSet<_>>().len() != symbols.len()
            || !seen.insert(symbols.clone())
        {
            continue;
        }
        let Some(displacements) = symbols
            .iter()
            .map(|name| offsets.get(name).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let full_width = displacements
            .iter()
            .any(|offset| i16::try_from(*offset).is_err());
        // Wide recipes contain final immediates, so each removed symbol reference
        // must already be owned by the source-order discovery stream. Removing
        // its relocation then cannot change the layout used to select those bits.
        if full_width
            && (!function.share_wide_bss_cursor_bases
                || symbols
                    .iter()
                    .any(|name| !function.symbol_order.contains(name)))
        {
            continue;
        }
        let mut start = 0;
        while start + 2 * symbols.len() <= function.instructions.len() {
            if let Some((destinations, temporaries)) = packet(function, start, &symbols) {
                let replacement = if full_width {
                    wide::plan(
                        &displacements,
                        &destinations,
                        &temporaries,
                        function.share_bss_page_expressions,
                    )
                } else {
                    narrow_packet(&symbols, &destinations, temporaries[0])
                };
                if let Some(length) = replace(function, start, 2 * symbols.len(), replacement) {
                    start += length;
                    continue;
                }
            }
            start += 1;
        }
    }
}

fn packet(
    function: &MachineFunction,
    start: usize,
    symbols: &[String],
) -> Option<(Vec<u8>, Vec<u8>)> {
    let end = start + 2 * symbols.len();
    let interior = |at: usize| start < at && at < end;
    if function.entry_points.iter().any(|(_, at)| interior(*at))
        || function.instructions.iter().any(|i| match i {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => interior(*target),
            _ => false,
        })
        || function
            .deferred_displacements
            .iter()
            .any(|d| (start..end).contains(&d.instruction_index))
    {
        return None;
    }
    let mut destinations = Vec::new();
    for (index, symbol) in symbols.iter().enumerate() {
        let high = start + 2 * index;
        let low = high + 1;
        let Instruction::AddImmediateShifted {
            d,
            a: 0,
            immediate: 0,
        } = function.instructions[high]
        else {
            return None;
        };
        if !(14..=31).contains(&d)
            || destinations.contains(&d)
            || !matches!(function.instructions[low], Instruction::AddImmediate { d: result, a, immediate: 0 } if result == d && a == d)
        {
            return None;
        }
        for (at, kind) in [
            (high, RelocationKind::Addr16Ha),
            (low, RelocationKind::Addr16Lo),
        ] {
            let mut owners = function
                .relocations
                .iter()
                .filter(|r| r.instruction_index == at);
            let r = owners.next()?;
            if owners.next().is_some()
                || r.kind != kind
                || !matches!(&r.target, RelocationTarget::External(name) if name == symbol)
            {
                return None;
            }
        }
        destinations.push(d);
    }
    let liveness = mwcc_vreg::analyze(&function.instructions);
    let temporaries: Vec<_> = (3..=12)
        .filter(|register| {
            !liveness.pinned.iter().any(|p| {
                p.class == mwcc_vreg::Class::General
                    && p.register == *register
                    && p.live_slots
                        .as_ref()
                        .map_or(p.start < end && p.end >= start, |slots| {
                            let first = slots.partition_point(|slot| *slot < 2 * start);
                            slots.get(first).is_some_and(|slot| *slot < 2 * end)
                        })
            })
        })
        .collect();
    (!temporaries.is_empty()).then_some((destinations, temporaries))
}

/// A complete section-base definition followed by cursor address definitions.
/// Narrow recipes retain layout fixups; full-width recipes use proven offsets.
struct AddressPacket {
    instructions: Vec<Instruction>,
    displacements: Vec<(usize, String)>,
}

fn narrow_packet(symbols: &[String], destinations: &[u8], temporary: u8) -> AddressPacket {
    let mut instructions = vec![
        Instruction::AddImmediateShifted {
            d: temporary,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: temporary,
            a: temporary,
            immediate: 0,
        },
    ];
    for d in destinations {
        instructions.push(Instruction::AddImmediate {
            d: *d,
            a: temporary,
            immediate: 0,
        });
    }
    AddressPacket {
        instructions,
        displacements: symbols
            .iter()
            .enumerate()
            .map(|(i, name)| (2 + i, name.clone()))
            .collect(),
    }
}

fn replace(
    function: &mut MachineFunction,
    start: usize,
    old_length: usize,
    packet: AddressPacket,
) -> Option<usize> {
    let end = start + old_length;
    let length = packet.instructions.len();
    let new_end = start + length;
    // No interior instruction owns a surviving relocation, fixup, or entry.
    // Control-flow targets at the packet start still enter its new definition.
    let remap = |at: usize| if at >= end { at - end + new_end } else { at };
    // A wide recipe can grow. Decline if moving either endpoint would exceed
    // an existing branch encoding; shrinking packets automatically satisfy it.
    if length > old_length
        && function.instructions.iter().enumerate().any(|(at, i)| {
            let (target, limit) = match i {
                Instruction::BranchConditionalForward { target, .. } => (*target, 1i64 << 15),
                Instruction::Branch { target } => (*target, 1i64 << 25),
                _ => return false,
            };
            let displacement = (remap(target) as i64 - remap(at) as i64) * 4;
            !(-limit..limit).contains(&displacement)
        })
    {
        return None;
    }
    function
        .relocations
        .retain(|r| !(start..end).contains(&r.instruction_index));
    for r in &mut function.relocations {
        r.instruction_index = remap(r.instruction_index);
    }
    for d in &mut function.deferred_displacements {
        d.instruction_index = remap(d.instruction_index);
    }
    for (_, at) in &mut function.entry_points {
        *at = remap(*at);
    }
    function
        .instructions
        .splice(start..end, packet.instructions);
    for i in &mut function.instructions {
        match i {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => *target = remap(*target),
            _ => {}
        }
    }
    for (at, kind) in [
        (start, RelocationKind::Addr16Ha),
        (start + 1, RelocationKind::Addr16Lo),
    ] {
        function.relocations.push(Relocation {
            instruction_index: at,
            kind,
            target: RelocationTarget::External("...bss.0".into()),
        });
    }
    for (at, symbol) in packet.displacements {
        function.deferred_displacements.push(DeferredDisplacement {
            instruction_index: start + at,
            target: DeferredDisplacementTarget::Symbol(symbol),
        });
    }
    function.relocations.sort_by_key(|r| r.instruction_index);
    function
        .deferred_displacements
        .sort_by_key(|d| d.instruction_index);
    Some(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> MachineFunction {
        let mut f = MachineFunction::new("setup");
        f.instructions.push(Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 7,
        });
        for (index, name) in ["a", "b", "c"].into_iter().enumerate() {
            let d = 31 - index as u8;
            let at = f.instructions.len();
            f.instructions.extend([
                Instruction::AddImmediateShifted {
                    d,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d,
                    a: d,
                    immediate: 0,
                },
            ]);
            for (instruction_index, kind) in [
                (at, RelocationKind::Addr16Ha),
                (at + 1, RelocationKind::Addr16Lo),
            ] {
                f.relocations.push(Relocation {
                    instruction_index,
                    kind,
                    target: RelocationTarget::External(name.into()),
                });
            }
        }
        // Incoming r3 must survive the packet even though it has no operand
        // there. Liveness, rather than an operand-only scan, excludes it.
        f.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        f.instructions.push(Instruction::BranchAndLink {
            target: "observe".into(),
        });
        f.relocations.push(Relocation {
            instruction_index: 8,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("observe".into()),
        });
        f.instructions.push(Instruction::Branch { target: 1 });
        f.entry_points.push(("after_setup".into(), 7));
        f.temporary_bss_address_groups
            .push(vec!["a".into(), "b".into(), "c".into()]);
        f
    }

    fn offsets(last: u32) -> HashMap<String, u32> {
        [("a".into(), 0), ("b".into(), 320), ("c".into(), last)].into()
    }

    #[test]
    fn preserves_live_inputs_control_flow_and_instruction_owners() {
        let mut f = setup();
        share(&mut f, &offsets(640));
        assert_eq!(f.instructions.len(), 9);
        assert!(matches!(
            f.instructions[1],
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0
            }
        ));
        for (index, d) in [31, 30, 29].into_iter().enumerate() {
            assert!(
                matches!(f.instructions[3 + index], Instruction::AddImmediate { d: result, a: 4, immediate: 0 } if result == d)
            );
        }
        assert!(matches!(
            f.instructions[0],
            Instruction::BranchConditionalForward { target: 6, .. }
        ));
        assert!(matches!(
            f.instructions[8],
            Instruction::Branch { target: 1 }
        ));
        assert_eq!(f.entry_points, [("after_setup".into(), 6)]);
        assert_eq!(
            f.relocations
                .iter()
                .map(|r| r.instruction_index)
                .collect::<Vec<_>>(),
            [1, 2, 7]
        );
        assert_eq!(
            f.deferred_displacements
                .iter()
                .map(|d| d.instruction_index)
                .collect::<Vec<_>>(),
            [3, 4, 5]
        );
        let once = f.instructions.clone();
        share(&mut f, &offsets(640));
        assert_eq!(f.instructions, once);
    }

    #[test]
    fn growing_wide_packets_remap_owners_and_preserve_branch_encoding_limits() {
        let mut f = setup();
        f.symbol_order = vec!["a".into(), "b".into(), "c".into()];
        f.share_wide_bss_cursor_bases = true;
        let offsets = [
            ("a".into(), 32768),
            ("b".into(), 33088),
            ("c".into(), 33408),
        ]
        .into();
        let mut boundary = f.clone();
        share(&mut f, &offsets);
        assert_eq!(f.instructions.len(), 12);
        assert!(matches!(
            f.instructions[0],
            Instruction::BranchConditionalForward { target: 9, .. }
        ));
        assert_eq!(f.entry_points[0].1, 9);
        assert_eq!(f.relocations.last().unwrap().instruction_index, 10);
        assert!(matches!(
            f.instructions[11],
            Instruction::Branch { target: 1 }
        ));
        assert!(f.deferred_displacements.is_empty());
        boundary.instructions.resize(
            8192,
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
        );
        boundary.instructions[0] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 8191,
        };
        let before = boundary.instructions.clone();
        share(&mut boundary, &offsets);
        assert_eq!(boundary.instructions, before);
    }

    #[test]
    fn full_width_immediates_require_existing_symbol_discovery_ownership() {
        let mut f = setup();
        f.share_wide_bss_cursor_bases = true;
        let before = f.instructions.clone();
        share(&mut f, &offsets(65536));
        assert_eq!(f.instructions, before);
    }

    #[test]
    fn requires_complete_signed_offsets_and_distinct_bases() {
        for (last, expected) in [(32764, 9), (32768, 10), (65536, 10)] {
            let mut f = setup();
            share(&mut f, &offsets(last));
            assert_eq!(f.instructions.len(), expected);
        }
        let mut missing = setup();
        share(&mut missing, &HashMap::new());
        assert_eq!(missing.instructions.len(), 10);
        let mut duplicate = setup();
        duplicate.temporary_bss_address_groups[0][2] = "a".into();
        share(&mut duplicate, &offsets(640));
        assert_eq!(duplicate.instructions.len(), 10);
    }

    #[test]
    fn declines_interior_entries_conflicting_fixups_and_register_pressure() {
        for variant in 0..5 {
            let mut f = setup();
            match variant {
                0 => f.instructions[0] = Instruction::Branch { target: 2 },
                1 => f.entry_points.push(("inside".into(), 4)),
                2 => f.deferred_displacements.push(DeferredDisplacement {
                    instruction_index: 3,
                    target: DeferredDisplacementTarget::Symbol("other".into()),
                }),
                3 => f.relocations[1].target = RelocationTarget::ExternalWithAddend("a".into(), 4),
                _ => {
                    // All volatile inputs are live before their first uses.
                    f.instructions.splice(
                        7..7,
                        (4..=12).map(|s| Instruction::StoreWord { s, a: 1, offset: 8 }),
                    );
                    f.relocations.last_mut().unwrap().instruction_index += 9;
                }
            }
            let original = f.instructions.clone();
            share(&mut f, &offsets(640));
            assert_eq!(f.instructions, original, "variant {variant}");
        }
    }
}
