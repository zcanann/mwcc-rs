//! Publish a leading value and fill constant while preparing the first CTR fill.
//!
//! Memory-dependent quotients issue their publication sooner than ready values.
//! The patched early scheduler also brings quotient-dependent address setup
//! forward. This pass permutes a proven packet; it never reorders its stores.

use crate::generator::Generator;
use mwcc_machine_code::{
    DeferredDisplacementTarget, Instruction as I, MachineFunction, RelocationKind,
};
use mwcc_versions::{FixedFillAddressPlacement, FrameConvention, Optimization, OptimizationGoal};
use mwcc_vreg::{register_operands, Class, RegisterRole};

impl Generator {
    pub(crate) fn schedule_fixed_fill_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization < Optimization::O3
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.preceded_by_asm
        {
            return;
        }
        // Layout expands wide address halves after this stage. Carry the
        // selected early scheduler policy without retaining instruction indices.
        self.output.following_fixed_fill_schedule = Some(
            match self.behavior.fixed_fill_address_placement {
                FixedFillAddressPlacement::AfterCountRegister => {
                    mwcc_machine_code::FixedFillAddressSchedule::AfterCountRegister
                }
                FixedFillAddressPlacement::BeforePublishedValues => {
                    mwcc_machine_code::FixedFillAddressSchedule::BeforeCountRegister
                }
            },
        );
        let Some(anchor) = &self.data_section_anchor else {
            return;
        };
        let Some(start) = self
            .output
            .instructions
            .iter()
            .position(|i| matches!(i, I::MoveToCountRegister { .. }))
            .and_then(|i| i.checked_sub(5))
        else {
            return;
        };
        if let Some(order) = plan(
            &self.output,
            start,
            &anchor.symbols,
            self.behavior.fixed_fill_address_placement,
        ) {
            crate::permute_machine_function_region(&mut self.output, start, &order);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishedValue {
    MemoryQuotient,
    RegisterQuotient,
    Literal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operand {
    Memory,
    Incoming,
    Constant,
}

fn defines(instruction: &I, register: u8) -> bool {
    register_operands(instruction).iter().any(|o| {
        o.class == Class::General && o.role == RegisterRole::Define && o.register == register
    })
}

fn operand(instructions: &[I], register: u8) -> Option<Operand> {
    let Some(at) = instructions.iter().rposition(|i| defines(i, register)) else {
        return Some(Operand::Incoming);
    };
    match instructions[at] {
        I::LoadWord { .. } => Some(Operand::Memory),
        I::AddImmediate { a: 0, .. } | I::AddImmediateShifted { a: 0, .. } => {
            Some(Operand::Constant)
        }
        I::AddImmediate { a, .. } => {
            (operand(&instructions[..at], a)? == Operand::Constant).then_some(Operand::Constant)
        }
        _ => None,
    }
}

fn published_value(instructions: &[I], result: u8) -> Option<PublishedValue> {
    let at = instructions.iter().rposition(|i| defines(i, result))?;
    match instructions[at] {
        I::AddImmediate { a: 0, .. } => Some(PublishedValue::Literal),
        I::ShiftRightLogicalImmediate { s, .. } => {
            let multiply = instructions[..at].iter().rposition(|i| defines(i, s))?;
            let I::MultiplyHighWordUnsigned { a, b, .. } = instructions[multiply] else {
                return None;
            };
            let origins = (
                operand(&instructions[..multiply], a)?,
                operand(&instructions[..multiply], b)?,
            );
            match origins {
                (Operand::Memory, Operand::Constant) | (Operand::Constant, Operand::Memory) => {
                    Some(PublishedValue::MemoryQuotient)
                }
                (Operand::Incoming, Operand::Constant) | (Operand::Constant, Operand::Incoming) => {
                    Some(PublishedValue::RegisterQuotient)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn plan(
    output: &MachineFunction,
    start: usize,
    anchor_symbols: &std::collections::HashSet<String>,
    placement: FixedFillAddressPlacement,
) -> Option<[usize; 6]> {
    let code = &output.instructions;
    if !output.jump_tables.is_empty()
        || !output.entry_points.is_empty()
        || code
            .iter()
            .any(|i| matches!(i, I::VerbatimWord(_) | I::BranchToCountRegister))
        || code[..start].iter().any(|i| {
            matches!(
                i,
                I::Branch { .. }
                    | I::BranchConditionalForward { .. }
                    | I::BranchAndLink { .. }
                    | I::BranchToCountRegisterAndLink
                    | I::BranchToLinkRegisterAndLink
            )
        })
        || code.iter().any(|i| match i {
            I::Branch { target } | I::BranchConditionalForward { target, .. } => {
                (start..start + 6).contains(target)
            }
            _ => false,
        })
    {
        return None;
    }
    let [I::StoreWord {
        s: result,
        a: 0,
        offset: 0,
    }, I::AddImmediate { d: fill, a: 0, .. }, I::StoreWord {
        s: published_fill,
        a: 0,
        offset: 0,
    }, I::AddImmediate {
        d: pointer,
        a: base @ 14..=31,
        ..
    }, I::AddImmediate {
        d: count,
        a: 0,
        immediate: iterations,
    }, I::MoveToCountRegister { s: ctr }] = code.get(start..start + 6)?
    else {
        return None;
    };
    if result != count
        || count != ctr
        || fill != published_fill
        || fill == result
        || pointer == fill
        || pointer == count
        || fill == base
        || result == base
        || *pointer == 0
        || *iterations < 2
    {
        return None;
    }
    let body = start + 6;
    let stores = code[body..].iter().take(10).enumerate().take_while(|(n, i)| matches!(i, I::StoreWord { s, a, offset } if s == fill && a == pointer && *offset == 4 * *n as i16)).count();
    if stores == 0
        || !matches!(code.get(body+stores), Some(I::AddImmediate { d, a, immediate }) if d == pointer && a == pointer && *immediate == 4 * stores as i16)
        || !matches!(code.get(body+stores+1), Some(I::BranchConditionalForward { options: 16, condition_bit: 0, target }) if *target == body)
    {
        return None;
    }
    let end = body + stores + 2;
    let relocations: Vec<_> = output
        .relocations
        .iter()
        .filter(|r| (start..end).contains(&r.instruction_index))
        .collect();
    if relocations.len() != 2
        || ![start, start + 2].iter().all(|&at| {
            relocations
                .iter()
                .any(|r| r.instruction_index == at && r.kind == RelocationKind::EmbSda21)
        })
    {
        return None;
    }
    let displacements: Vec<_> = output
        .deferred_displacements
        .iter()
        .filter(|d| (start..end).contains(&d.instruction_index))
        .collect();
    if displacements.len() != 1
        || displacements[0].instruction_index != start + 3
        || !matches!(&displacements[0].target, DeferredDisplacementTarget::SymbolAddress(name) if anchor_symbols.contains(name))
    {
        return None;
    }
    Some(schedule(
        published_value(&code[..start], *result)?,
        placement,
    ))
}

fn schedule(value: PublishedValue, placement: FixedFillAddressPlacement) -> [usize; 6] {
    use FixedFillAddressPlacement::*;
    use PublishedValue::*;
    match (value, placement) {
        (MemoryQuotient, AfterCountRegister) => [1, 0, 4, 2, 5, 3],
        (MemoryQuotient, BeforePublishedValues) => [3, 1, 0, 4, 2, 5],
        (RegisterQuotient, BeforePublishedValues) => [3, 1, 0, 4, 5, 2],
        (RegisterQuotient | Literal, _) => [1, 3, 0, 4, 5, 2],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{DeferredDisplacement, Relocation, RelocationTarget};

    fn memory_value() -> Vec<I> {
        vec![
            I::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: -32768,
            },
            I::LoadWord {
                d: 0,
                a: 4,
                offset: 248,
            },
            I::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0x51ec,
            },
            I::AddImmediate {
                d: 3,
                a: 3,
                immediate: -31457,
            },
            I::MultiplyHighWordUnsigned { d: 0, a: 3, b: 0 },
            I::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 7,
            },
        ]
    }

    fn fixture() -> MachineFunction {
        let mut output = MachineFunction::new("fill");
        output.instructions = memory_value();
        output.instructions.extend([
            I::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            I::load_immediate(3, 7),
            I::StoreWord {
                s: 3,
                a: 0,
                offset: 0,
            },
            I::AddImmediate {
                d: 4,
                a: 31,
                immediate: 0,
            },
            I::load_immediate(0, 8),
            I::MoveToCountRegister { s: 0 },
        ]);
        for n in 0..8 {
            output.instructions.push(I::StoreWord {
                s: 3,
                a: 4,
                offset: 4 * n,
            });
        }
        output.instructions.extend([
            I::AddImmediate {
                d: 4,
                a: 4,
                immediate: 32,
            },
            I::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 12,
            },
            I::BranchToLinkRegister,
        ]);
        for (instruction_index, name) in [(6, "result"), (8, "fill")] {
            output.relocations.push(Relocation {
                instruction_index,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External(name.into()),
            });
        }
        output.deferred_displacements.push(DeferredDisplacement {
            instruction_index: 9,
            target: DeferredDisplacementTarget::SymbolAddress("array".into()),
        });
        output
    }

    fn selected(
        output: &MachineFunction,
        placement: FixedFillAddressPlacement,
    ) -> Option<[usize; 6]> {
        plan(
            output,
            6,
            &std::collections::HashSet::from(["array".into()]),
            placement,
        )
    }

    #[test]
    fn distinguishes_loaded_quotients_incoming_quotients_and_literals() {
        assert_eq!(
            published_value(&memory_value(), 0),
            Some(PublishedValue::MemoryQuotient)
        );
        let incoming = [
            I::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0x51ec,
            },
            I::AddImmediate {
                d: 0,
                a: 4,
                immediate: -31457,
            },
            I::MultiplyHighWordUnsigned { d: 0, a: 0, b: 3 },
            I::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 7,
            },
        ];
        assert_eq!(
            published_value(&incoming, 0),
            Some(PublishedValue::RegisterQuotient)
        );
        assert_eq!(
            published_value(&[I::load_immediate(0, 123)], 0),
            Some(PublishedValue::Literal)
        );
        assert_eq!(
            published_value(
                &[I::LoadWord {
                    d: 0,
                    a: 3,
                    offset: 0
                }],
                0
            ),
            None
        );
    }

    #[test]
    fn schedules_preserve_publication_order_and_count_dependencies() {
        for value in [
            PublishedValue::MemoryQuotient,
            PublishedValue::RegisterQuotient,
            PublishedValue::Literal,
        ] {
            for placement in [
                FixedFillAddressPlacement::AfterCountRegister,
                FixedFillAddressPlacement::BeforePublishedValues,
            ] {
                let order = schedule(value, placement);
                let position = |old| order.iter().position(|&i| i == old).unwrap();
                assert!(position(0) < position(2));
                assert!(position(0) < position(4));
                assert!(position(1) < position(2));
                assert!(position(4) < position(5));
                let mut sorted = order;
                sorted.sort();
                assert_eq!(sorted, [0, 1, 2, 3, 4, 5]);
            }
        }
    }

    #[test]
    fn permutation_keeps_address_fixups_and_global_relocations_attached() {
        let mut output = fixture();
        let order = selected(&output, FixedFillAddressPlacement::AfterCountRegister).unwrap();
        assert_eq!(order, [1, 0, 4, 2, 5, 3]);
        crate::permute_machine_function_region(&mut output, 6, &order);
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|r| r.instruction_index)
                .collect::<Vec<_>>(),
            [7, 9]
        );
        assert_eq!(output.deferred_displacements[0].instruction_index, 11);
        assert!(matches!(
            output.instructions[21],
            I::BranchConditionalForward { target: 12, .. }
        ));
    }

    #[test]
    fn rejects_side_entries_barriers_aliases_and_unowned_state() {
        let placement = FixedFillAddressPlacement::AfterCountRegister;
        for target in 6..12 {
            let mut output = fixture();
            output.instructions.push(I::Branch { target });
            assert!(selected(&output, placement).is_none());
        }
        for (at, instruction) in [
            (
                0,
                I::BranchAndLink {
                    target: "barrier".into(),
                },
            ),
            (7, I::load_immediate(0, 7)),
            (
                9,
                I::AddImmediate {
                    d: 3,
                    a: 31,
                    immediate: 0,
                },
            ),
            (10, I::load_immediate(0, 1)),
            (
                12,
                I::StoreWord {
                    s: 0,
                    a: 4,
                    offset: 0,
                },
            ),
            (
                20,
                I::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 28,
                },
            ),
        ] {
            let mut output = fixture();
            output.instructions[at] = instruction;
            assert!(selected(&output, placement).is_none());
        }
        let mut output = fixture();
        output.relocations[1].instruction_index = 7;
        assert!(selected(&output, placement).is_none());
        let mut output = fixture();
        output.deferred_displacements[0].target =
            DeferredDisplacementTarget::SymbolAddress("unowned".into());
        assert!(selected(&output, placement).is_none());
    }
}
