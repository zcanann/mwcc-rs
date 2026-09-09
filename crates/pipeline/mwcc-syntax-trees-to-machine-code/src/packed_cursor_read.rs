//! Retain a nonvolatile member cursor across a signed packed-byte extraction.
//! The independent cursor update can fill the extraction's latency slots.
use crate::generator::Generator;
use crate::register_value_liveness::dead_after;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Type;
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn retain_packed_read_cursors(&mut self, return_type: Type) {
        if self.non_leaf
            || !matches!(return_type, Type::Int | Type::UnsignedInt | Type::Void)
            || !self.output.jump_tables.is_empty()
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
        {
            return;
        }
        let bases: Vec<_> = self
            .nonvolatile_pointer_bindings
            .iter()
            .filter_map(|name| self.locations.get(name).map(|location| location.register))
            .collect();
        let mut at = 0;
        while at + 7 <= self.output.instructions.len() {
            let Some(plan) = recognize(&self.output.instructions, at, &bases) else {
                at += 1;
                continue;
            };
            let mut load = self.output.instructions[at].clone();
            let mut byte = self.output.instructions[at + 1].clone();
            let mut shift = self.output.instructions[at + 2].clone();
            let mut signed = self.output.instructions[at + 3].clone();
            let mut update = self.output.instructions[at + 5].clone();
            let store = self.output.instructions[at + 6].clone();
            if self.behavior.schedule_latency_slots && self.behavior.scheduler_enabled {
                let direct_return = matches!(self.output.instructions.get(at + 7),
                    Some(Instruction::Or { a: 3, s, b }) if *s == plan.result && *b == plan.result);
                self.prefer_virtual_general(plan.result, if direct_return { 3 } else { 5 });
                let cursor = self.fresh_virtual_general_preferring(5);
                let bits = self.fresh_virtual_general_preferring(4);
                if let Instruction::LoadWord { d, .. } = &mut load {
                    *d = cursor;
                }
                if let Instruction::LoadByteZero { d, a, .. } = &mut byte {
                    *d = bits;
                    *a = cursor;
                }
                if let Instruction::ShiftLeftImmediate { a, s, .. }
                | Instruction::RotateAndMask { a, s, .. } = &mut shift
                {
                    *a = bits;
                    *s = bits;
                }
                if let Instruction::ShiftRightAlgebraicImmediate { s, .. } = &mut signed {
                    *s = bits;
                }
                if let Instruction::AddImmediate { a, .. } = &mut update {
                    *a = cursor;
                }
                self.output.instructions[at..at + 6]
                    .clone_from_slice(&[load, byte, update, shift, store, signed]);
            } else {
                if let Instruction::AddImmediate { a, .. } = &mut update {
                    *a = plan.pointer;
                }
                self.output.instructions[at..at + 6]
                    .clone_from_slice(&[load, byte, shift, signed, update, store]);
            }
            crate::remove_instruction_retargeting_to_next(self, at + 6);
            at += 6;
        }
    }
}

struct Plan {
    pointer: u32,
    result: u32,
}

fn recognize(instructions: &[Instruction], start: usize, bases: &[u32]) -> Option<Plan> {
    let [Instruction::LoadWord {
        d: pointer,
        a: base,
        offset,
    }, Instruction::LoadByteZero {
        d: bits,
        a: byte_base,
        ..
    }, transform, Instruction::ShiftRightAlgebraicImmediate {
        a: result,
        s: signed_source,
        ..
    }, Instruction::LoadWord {
        d: reload,
        a: reload_base,
        offset: reload_offset,
    }, Instruction::AddImmediate {
        d: updated,
        a: update_base,
        ..
    }, Instruction::StoreWord {
        s: published,
        a: store_base,
        offset: store_offset,
    }] = &instructions[start..start + 7]
    else {
        return None;
    };
    let (shifted, shift_source) = match transform {
        Instruction::ShiftLeftImmediate { a, s, .. } | Instruction::RotateAndMask { a, s, .. } => {
            (a, s)
        }
        _ => return None,
    };
    if !bases.contains(base)
        || pointer == bits
        || pointer == shifted
        || pointer != byte_base
        || bits != shift_source
        || shifted != signed_source
        || base != reload_base
        || base != store_base
        || offset != reload_offset
        || offset != store_offset
        || reload != update_base
        || updated != published
        || base == pointer
        || base == bits
        || base == shifted
        || base == updated
        || !mwcc_vreg::Reg::is_virtual_field(*result)
    {
        return None;
    }
    if [pointer, bits, shifted, reload, updated, base].contains(&result) {
        return None;
    }
    for register in [*pointer, *bits, *shifted, *reload] {
        if register != *updated && !dead_after(instructions, start + 7, register) {
            return None;
        }
    }
    // Reordering must not cross a second control-flow entry into the transaction.
    if instructions.iter().any(|instruction| match instruction {
        Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. } => {
            *target > start && *target < start + 7
        }
        _ => false,
    }) {
        return None;
    }
    Some(Plan {
        pointer: *pointer,
        result: *result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transaction() -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 8,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 4,
                offset: 0,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 27,
            },
            Instruction::ShiftRightAlgebraicImmediate {
                a: mwcc_vreg::VIRTUAL_BASE,
                s: 0,
                shift: 27,
            },
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 5,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 8,
            },
            Instruction::BranchToLinkRegister,
        ]
    }
    #[test]
    fn requires_nonvolatile_provenance_and_dead_cursor_temporaries() {
        let mut instructions = transaction();
        assert!(recognize(&instructions, 0, &[3]).is_some());
        assert!(recognize(&instructions, 0, &[]).is_none());
        instructions[7] = Instruction::move_register(3, 5);
        assert!(recognize(&instructions, 0, &[3]).is_none());
    }
    #[test]
    fn checks_both_successor_paths_and_rejects_interior_entries() {
        let mut instructions = transaction();
        instructions[7] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 9,
        };
        instructions.extend([
            Instruction::BranchToLinkRegister,
            Instruction::move_register(3, 4),
        ]);
        assert!(recognize(&instructions, 0, &[3]).is_none());
        let mut instructions = transaction();
        instructions.push(Instruction::Branch { target: 2 });
        assert!(recognize(&instructions, 0, &[3]).is_none());
    }
}
