//! Resolve complete BSS-relative addresses after unit layout, before debug encoding.
//!
//! The object writer and this pass share BSS ordering. Existing low-half fixups
//! remain low-half fixups, including accesses through explicitly biased pages.

use super::{DefinedGlobal, ObjectFormat};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{
    DeferredDisplacementTarget as Target, Instruction, MachineFunction, RelocationTarget,
};
use std::collections::HashMap;

pub fn finalize_bss_addresses(
    functions: &mut [MachineFunction],
    globals: &[DefinedGlobal],
    format: ObjectFormat,
    small_data: bool,
) -> Compilation<()> {
    if !functions.iter().any(|f| {
        f.deferred_displacements
            .iter()
            .any(|d| matches!(d.target, Target::SymbolAddress(_)))
    }) {
        return Ok(());
    }
    let names: Vec<_> = globals
        .iter()
        .enumerate()
        .map(|(index, global)| {
            if global.static_local_owner.is_some() {
                mwcc_object::static_local_link_name(&global.name, index)
            } else {
                global.name.clone()
            }
        })
        .collect();
    let link_name = |function, symbol: &str| {
        globals
            .iter()
            .enumerate()
            .find(|(_, g)| g.static_local_owner == Some(function) && g.name == symbol)
            .map_or_else(|| symbol.to_owned(), |(index, _)| names[index].clone())
    };
    let objects: Vec<_> = globals
        .iter()
        .zip(&names)
        .map(|(global, name)| global.object(name))
        .collect();
    let references: Vec<Vec<String>> = functions
        .iter()
        .enumerate()
        .map(|(index, function)| {
            function
                .symbol_order
                .iter()
                .cloned()
                .chain(function.relocations.iter().filter_map(|r| match &r.target {
                    RelocationTarget::External(name)
                    | RelocationTarget::ExternalWithAddend(name, _) => Some(link_name(index, name)),
                    _ => None,
                }))
                .chain(
                    function
                        .deferred_displacements
                        .iter()
                        .filter_map(|d| match &d.target {
                            Target::Symbol(name) | Target::SymbolAddress(name) => {
                                Some(link_name(index, name))
                            }
                            _ => None,
                        }),
                )
                .collect()
        })
        .collect();
    let mut offsets = HashMap::new();
    let mut cursor = 0u32;
    for index in mwcc_object::bss_object_order(&objects, &references, small_data, format) {
        let object = &objects[index];
        let align = object.alignment.max(1);
        cursor = cursor
            .div_ceil(align)
            .checked_mul(align)
            .ok_or_else(|| Diagnostic::error("BSS layout exceeds the target address space"))?;
        offsets.insert(names[index].clone(), cursor);
        cursor = cursor
            .checked_add(object.size)
            .ok_or_else(|| Diagnostic::error("BSS layout exceeds the target address space"))?;
    }
    for (index, function) in functions.iter_mut().enumerate() {
        let resolved: Vec<_> = function
            .deferred_displacements
            .iter()
            .enumerate()
            .filter_map(|(fixup, d)| {
                let Target::SymbolAddress(name) = &d.target else {
                    return None;
                };
                offsets
                    .get(&link_name(index, name))
                    .map(|offset| (fixup, *offset))
            })
            .collect();
        expand(function, &resolved)?;
    }
    Ok(())
}

fn expand(function: &mut MachineFunction, resolved: &[(usize, u32)]) -> Compilation<()> {
    let mut additions = HashMap::new();
    for &(fixup, offset) in resolved {
        let at = function.deferred_displacements[fixup].instruction_index;
        let Instruction::AddImmediate { d, a, immediate } = function.instructions[at] else {
            return Err(Diagnostic::error(format!(
                "complete section address must own an addi (instruction {at}: {:?})",
                function.instructions[at],
            )));
        };
        let displacement = offset.wrapping_add(immediate as i32 as u32) as i32;
        if i16::try_from(displacement).is_ok() {
            continue;
        }
        if a == 0
            || function.is_asm
            || function
                .instructions
                .iter()
                .any(|i| matches!(i, Instruction::VerbatimWord { .. }))
        {
            return Err(Diagnostic::error(
                "large section address needs an ordinary register-based instruction stream",
            ));
        }
        let high = ((i64::from(displacement) + 0x8000) >> 16) as i16;
        let temp = if d != 0 {
            d
        } else {
            let liveness = mwcc_vreg::analyze(&function.instructions);
            (3..=12)
                .find(|r| {
                    *r != a
                        && !liveness.pinned.iter().any(|p| {
                            p.class == mwcc_vreg::Class::General
                                && p.register == *r
                                && p.live_slots.as_ref().map_or(
                                    p.start <= at && at <= p.end,
                                    |slots| {
                                        slots.binary_search(&(2 * at)).is_ok()
                                            || slots.binary_search(&(2 * at + 1)).is_ok()
                                    },
                                )
                        })
                })
                .ok_or_else(|| {
                    Diagnostic::error("large section address has no free high-half register")
                })?
        };
        additions.insert(at, (temp, a, high));
    }
    if !additions.is_empty() {
        let old = std::mem::take(&mut function.instructions);
        let mut starts = Vec::with_capacity(old.len() + 1);
        let mut owners = Vec::with_capacity(old.len());
        for (at, mut instruction) in old.into_iter().enumerate() {
            starts.push(function.instructions.len());
            if let Some(&(temp, base, high)) = additions.get(&at) {
                function
                    .instructions
                    .push(Instruction::AddImmediateShifted {
                        d: temp,
                        a: base,
                        immediate: high,
                    });
                if let Instruction::AddImmediate { a, .. } = &mut instruction {
                    *a = temp;
                }
            }
            owners.push(function.instructions.len());
            function.instructions.push(instruction);
        }
        starts.push(function.instructions.len());
        for r in &mut function.relocations {
            r.instruction_index = owners[r.instruction_index];
        }
        for d in &mut function.deferred_displacements {
            d.instruction_index = owners[d.instruction_index];
        }
        for (_, at) in &mut function.entry_points {
            *at = starts[*at];
        }
        for table in &mut function.jump_tables {
            for entry in &mut table.entries {
                *entry = 4 * starts[*entry as usize / 4] as u32;
            }
        }
        for (at, instruction) in function.instructions.iter_mut().enumerate() {
            let (target, limit) = match instruction {
                Instruction::BranchConditionalForward { target, .. } => (target, 1i64 << 15),
                Instruction::Branch { target } => (target, 1i64 << 25),
                _ => continue,
            };
            *target = starts[*target];
            let displacement = (*target as i64 - at as i64) * 4;
            if !(-limit..limit).contains(&displacement) {
                return Err(Diagnostic::error(
                    "section address expansion exceeds branch range",
                ));
            }
        }
    }
    // Keep the low fixup (and its symbol-discovery event) for the writer. Mark
    // it consumed so repeating finalization cannot add the high half twice.
    for &(fixup, _) in resolved {
        if let Target::SymbolAddress(name) = &function.deferred_displacements[fixup].target {
            function.deferred_displacements[fixup].target = Target::Symbol(name.clone());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{DeferredDisplacement, JumpTable, Relocation, RelocationKind};

    fn function(destination: u8) -> MachineFunction {
        let mut f = MachineFunction::new("address");
        f.instructions = vec![
            Instruction::Branch { target: 1 },
            Instruction::AddImmediate {
                d: destination,
                a: 31,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: destination,
                a: 31,
                offset: 0,
            },
            Instruction::BranchToLinkRegister,
        ];
        f.deferred_displacements.push(DeferredDisplacement {
            instruction_index: 1,
            target: Target::SymbolAddress("buffer".into()),
        });
        f
    }

    #[test]
    fn expands_signed_low_boundaries_and_keeps_the_anchor_intact() {
        for (offset, high) in [
            (0x7fff, None),
            (0x8000, Some(1)),
            (0xffff, Some(1)),
            (0x10000, Some(1)),
            (0x17fff, Some(1)),
            (0x18000, Some(2)),
        ] {
            let mut f = function(3);
            expand(&mut f, &[(0, offset)]).unwrap();
            assert_eq!(f.instructions.len(), if high.is_some() { 5 } else { 4 });
            if let Some(high) = high {
                assert!(
                    matches!(f.instructions[1], Instruction::AddImmediateShifted { d: 3, a: 31, immediate } if immediate == high)
                );
            }
            assert!(matches!(
                f.instructions[0],
                Instruction::Branch { target: 1 }
            ));
        }
    }

    #[test]
    fn instruction_owners_and_control_flow_have_distinct_expansion_positions() {
        let mut f = function(3);
        f.entry_points.push(("entry".into(), 1));
        f.jump_tables.push(JumpTable {
            entries: vec![4, 16],
            anonymous_offset: 0,
        });
        f.relocations.push(Relocation {
            instruction_index: 2,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External("other".into()),
        });
        expand(&mut f, &[(0, 0x8d00)]).unwrap();
        assert_eq!(f.entry_points[0].1, 1);
        assert_eq!(f.jump_tables[0].entries, [4, 20]);
        assert_eq!(f.relocations[0].instruction_index, 3);
        assert_eq!(f.deferred_displacements[0].instruction_index, 2);
    }

    #[test]
    fn scratch_results_preserve_live_call_arguments() {
        let mut f = function(0);
        f.instructions
            .insert(3, Instruction::Add { d: 3, a: 3, b: 4 });
        expand(&mut f, &[(0, 0x8d00)]).unwrap();
        assert!(matches!(
            f.instructions[1],
            Instruction::AddImmediateShifted {
                d: 5,
                a: 31,
                immediate: 1
            }
        ));
        assert!(matches!(
            f.instructions[2],
            Instruction::AddImmediate { d: 0, a: 5, .. }
        ));
    }
}
