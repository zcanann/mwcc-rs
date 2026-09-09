//! Schedule retained member cursors and independent word updates.
//!
//! Recognition supplies memory and lifetime proofs. Issue order and register
//! preferences are separate decisions; byte reads stay ordered with narrow
//! stores because the pointed bytes can alias those fields.
use crate::generator::Generator;
use crate::register_value_liveness::dead_after;
use mwcc_machine_code::Instruction as I;
use mwcc_syntax_trees::Type;
use mwcc_versions::MemberValueSchedule;
use mwcc_vreg::{Class, RegisterRole};

struct Packet {
    load: usize,
    store: usize,
}

struct Plan {
    start: usize,
    end: usize,
    cursor: u32,
    loaded: bool,
    initial_count: Option<usize>,
    packets: Vec<Packet>,
    cursor_step: usize,
    count_step: Option<usize>,
}

fn transform(i: &I) -> bool {
    matches!(
        i,
        I::RotateAndMask { a: 0, s: 0, .. }
            | I::AndContiguousMask { a: 0, s: 0, .. }
            | I::ClearLeftImmediate { a: 0, s: 0, .. }
            | I::ShiftLeftImmediate { a: 0, s: 0, .. }
            | I::ShiftRightAlgebraicImmediate { a: 0, s: 0, .. }
    )
}

fn narrow_store(i: &I) -> Option<(u32, i16, i32)> {
    match i {
        I::StoreByte { s: 0, a, offset } => Some((*a, *offset, 1)),
        I::StoreHalfword { s: 0, a, offset } => Some((*a, *offset, 2)),
        _ => None,
    }
}

fn disjoint(offset: i16, width: i32, word: i16) -> bool {
    i32::from(offset) + width <= i32::from(word) || i32::from(word) + 4 <= i32::from(offset)
}

fn branch_target(i: &I) -> Option<usize> {
    match i {
        I::Branch { target } | I::BranchConditionalForward { target, .. } => Some(*target),
        _ => None,
    }
}

fn defines(i: &I, register: u32) -> bool {
    mwcc_vreg::register_operands(i).iter().any(|o| {
        o.class == Class::General && o.role == RegisterRole::Define && o.register == register
    })
}

fn recognize(code: &[I], first: usize, bases: &[u32]) -> Option<Plan> {
    let I::LoadByteZero {
        d: 0, a: cursor, ..
    } = *code.get(first)?
    else {
        return None;
    };
    if cursor == 0 {
        return None;
    }
    let mut at = first;
    let mut packets = Vec::new();
    while matches!(code.get(at), Some(I::LoadByteZero { d: 0, a, .. }) if *a == cursor) {
        let load = at;
        at += 1;
        while code.get(at).is_some_and(transform) {
            at += 1;
        }
        narrow_store(code.get(at)?)?;
        packets.push(Packet { load, store: at });
        at += 1;
    }
    let cursor_step = at;
    if !matches!(code.get(at), Some(I::AddImmediate { d: 0, a, .. }) if *a == cursor) {
        return None;
    }
    let I::StoreWord {
        s: 0,
        a: base,
        offset,
    } = *code.get(at + 1)?
    else {
        return None;
    };
    if !bases.contains(&base) || cursor == base {
        return None;
    }
    at += 2;
    let count_step = match (code.get(at), code.get(at + 1)) {
        (
            Some(I::AddImmediate { d: 0, a, .. }),
            Some(I::StoreWord {
                s: 0,
                a: b,
                offset: o,
            }),
        ) if *a != 0 && *a != cursor && *a != base && *b == base && disjoint(*o, 4, offset) => {
            at += 2;
            Some(at - 2)
        }
        _ => None,
    };
    for packet in &packets {
        let (a, o, width) = narrow_store(&code[packet.store])?;
        if a != base || !disjoint(o, width, offset) {
            return None;
        }
    }
    let mut start = first.checked_sub(1)?;
    let mut initial_count = None;
    if start >= 2
        && matches!(code[start - 1], I::AddImmediate { d: 0, a: 0, .. })
        && matches!(code[start], I::StoreWord { s: 0, a, offset: o } if a == base && disjoint(o, 4, offset))
    {
        initial_count = Some(start - 1);
        start -= 2;
    }
    let loaded = match code[start] {
        I::LoadWord { d, a, offset: o }
            if d == cursor
                && a == base
                && o == offset
                && mwcc_vreg::Reg::is_virtual_field(cursor)
                && initial_count.is_none() =>
        {
            true
        }
        I::StoreWord { s, a, offset: o } if s == cursor && a == base && o == offset => false,
        _ => return None,
    };
    // Initialization and a later counter increment have different live roots.
    if !loaded && count_step.is_some() {
        return None;
    }
    if code[..first].iter().any(|i| defines(i, base))
        || code
            .iter()
            .any(|i| branch_target(i).is_some_and(|t| t > start && t < at))
        || !dead_after(code, at, 0)
    {
        return None;
    }
    Some(Plan {
        start,
        end: at,
        cursor,
        loaded,
        initial_count,
        packets,
        cursor_step,
        count_step,
    })
}

fn issue_order(p: &Plan, style: MemberValueSchedule) -> Vec<usize> {
    let mut order = Vec::new();
    if let Some(count) = p.initial_count {
        order.push(count);
    }
    order.push(p.start);
    if !p.loaded {
        order.push(p.cursor_step);
    }
    if let Some(count) = p.initial_count {
        order.push(count + 1);
    }
    if let Some(count) = p.count_step {
        order.push(count);
    }
    let early_store = matches!(style, MemberValueSchedule::ReadyValues { ordered_issue_width } if ordered_issue_width > 1);
    for (index, packet) in p.packets.iter().enumerate() {
        order.push(packet.load);
        if index == 0 && p.loaded {
            order.push(p.cursor_step);
        }
        order.extend(packet.load + 1..packet.store);
        let last = index + 1 == p.packets.len();
        let exchange = last && early_store && packet.store > packet.load + 1;
        if exchange {
            order.push(p.cursor_step + 1);
        }
        order.push(packet.store);
        if last && !exchange {
            order.push(p.cursor_step + 1);
        }
    }
    if let Some(count) = p.count_step {
        order.push(count + 1);
    }
    order
}

fn rename_zero(i: &mut I, register: u32) {
    mwcc_vreg::for_each_register(i, |_, class, r| {
        if class == Class::General && *r == 0 {
            *r = register;
        }
    });
}

impl Generator {
    pub(crate) fn schedule_member_reads(&mut self, return_type: Type) {
        if !self.behavior.retain_disjoint_member_reads
            || !self.behavior.schedule_latency_slots
            || !self.behavior.scheduler_enabled
            || !matches!(return_type, Type::Int | Type::UnsignedInt | Type::Void)
            || self.non_leaf
            || self.output.is_asm
            || !self.output.entry_points.is_empty()
            || !self.output.jump_tables.is_empty()
            || !self.output.relocations.is_empty()
            || !self.output.deferred_displacements.is_empty()
            || self
                .output
                .instructions
                .iter()
                .any(|i| matches!(i, I::VerbatimWord(_)))
        {
            return;
        }
        let bases = self
            .nonvolatile_pointer_bindings
            .iter()
            .filter_map(|name| self.locations.get(name).map(|l| l.register))
            .collect::<Vec<_>>();
        let mut at = 0;
        while at < self.output.instructions.len() {
            let Some(p) = recognize(&self.output.instructions, at, &bases) else {
                at += 1;
                continue;
            };
            let counted = p.count_step.is_some();
            let single = p.packets.len() == 1;
            if p.loaded {
                self.prefer_virtual_general(
                    p.cursor,
                    if counted {
                        if single {
                            4
                        } else {
                            6
                        }
                    } else {
                        5
                    },
                );
            }
            if let Some(count) = p.count_step {
                if let I::AddImmediate { a, .. } = self.output.instructions[count] {
                    self.prefer_virtual_general(a, if single { 5 } else { 4 });
                }
                let value = self.fresh_virtual_general_preferring(0);
                rename_zero(&mut self.output.instructions[count], value);
                rename_zero(&mut self.output.instructions[count + 1], value);
            }
            if let Some(count) = p.initial_count {
                let value = self.fresh_virtual_general_preferring(5);
                rename_zero(&mut self.output.instructions[count], value);
                rename_zero(&mut self.output.instructions[count + 1], value);
            }
            let step = self.fresh_virtual_general_preferring(if counted { 4 } else { 0 });
            rename_zero(&mut self.output.instructions[p.cursor_step], step);
            rename_zero(&mut self.output.instructions[p.cursor_step + 1], step);
            for (index, packet) in p.packets.iter().enumerate() {
                let preference = if !p.loaded && index + 1 == p.packets.len() {
                    4
                } else if counted || !p.loaded {
                    5
                } else {
                    4
                };
                let byte = self.fresh_virtual_general_preferring(preference);
                for i in &mut self.output.instructions[packet.load..=packet.store] {
                    rename_zero(i, byte);
                }
            }
            let order = issue_order(&p, self.behavior.member_value_schedule);
            apply_order(&mut self.output, &p, &order);
            at = p.end;
        }
    }
}

fn apply_order(output: &mut mwcc_machine_code::MachineFunction, p: &Plan, order: &[usize]) {
    // A branch to the old block head must execute the new first value.
    for i in &mut output.instructions {
        match i {
            I::Branch { target } | I::BranchConditionalForward { target, .. }
                if *target == p.start =>
            {
                *target = order[0]
            }
            _ => {}
        }
    }
    let relative: Vec<_> = order.iter().map(|i| i - p.start).collect();
    crate::permute_machine_function_region(output, p.start, &relative);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn transaction() -> Vec<I> {
        vec![
            I::LoadWord {
                d: 32,
                a: 3,
                offset: 0,
            },
            I::LoadByteZero {
                d: 0,
                a: 32,
                offset: 0,
            },
            I::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 26,
                end: 31,
            },
            I::StoreByte {
                s: 0,
                a: 3,
                offset: 8,
            },
            I::LoadByteZero {
                d: 0,
                a: 32,
                offset: 0,
            },
            I::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 28,
                end: 31,
            },
            I::StoreHalfword {
                s: 0,
                a: 3,
                offset: 10,
            },
            I::AddImmediate {
                d: 0,
                a: 32,
                immediate: 3,
            },
            I::StoreWord {
                s: 0,
                a: 3,
                offset: 0,
            },
            I::BranchToLinkRegister,
        ]
    }
    #[test]
    fn keeps_aliased_byte_reads_ordered_and_uses_the_measured_store_issue_width() {
        let code = transaction();
        let p = recognize(&code, 1, &[3]).unwrap();
        assert_eq!(
            issue_order(
                &p,
                MemberValueSchedule::ReadyValues {
                    ordered_issue_width: 2
                }
            ),
            vec![0, 1, 7, 2, 3, 4, 5, 8, 6]
        );
        assert_eq!(
            issue_order(
                &p,
                MemberValueSchedule::ReadyValues {
                    ordered_issue_width: 1
                }
            ),
            vec![0, 1, 7, 2, 3, 4, 5, 6, 8]
        );
        assert!(recognize(&code, 1, &[]).is_none());
        let mut code = code;
        code[6] = I::StoreHalfword {
            s: 0,
            a: 3,
            offset: 3,
        };
        assert!(recognize(&code, 1, &[3]).is_none());
    }
    #[test]
    fn rejects_interior_entries_and_live_scratch_results() {
        let mut code = transaction();
        code.push(I::Branch { target: 4 });
        assert!(recognize(&code, 1, &[3]).is_none());
        let mut code = transaction();
        code.insert(9, I::move_register(3, 0));
        assert!(recognize(&code, 1, &[3]).is_none());
        let mut code = transaction();
        code.insert(
            4,
            I::StoreWord {
                s: 4,
                a: 5,
                offset: 0,
            },
        );
        assert!(recognize(&code, 1, &[3]).is_none());
    }
    #[test]
    fn an_initialization_entry_executes_the_hoisted_constant() {
        let mut code = transaction();
        code[0] = I::StoreWord {
            s: 32,
            a: 3,
            offset: 0,
        };
        code.splice(
            1..1,
            [
                I::load_immediate(0, 7),
                I::StoreWord {
                    s: 0,
                    a: 3,
                    offset: 4,
                },
            ],
        );
        code.push(I::Branch { target: 0 });
        let p = recognize(&code, 3, &[3]).unwrap();
        let order = issue_order(
            &p,
            MemberValueSchedule::ReadyValues {
                ordered_issue_width: 2,
            },
        );
        let mut output = mwcc_machine_code::MachineFunction {
            instructions: code,
            ..Default::default()
        };
        apply_order(&mut output, &p, &order);
        assert_eq!(output.instructions[0], I::load_immediate(0, 7));
        assert_eq!(output.instructions.last(), Some(&I::Branch { target: 0 }));
    }
}
