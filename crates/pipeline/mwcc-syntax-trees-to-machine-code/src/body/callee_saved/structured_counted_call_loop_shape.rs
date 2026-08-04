//! Physical bounds of a call-making counted loop.
//!
//! Late invariant schedulers share this shape after source loops have become
//! resolved branches: the backedge target is the body head, while zero-valued
//! definitions of the stepped induction registers mark the preheader start.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::MachineFunction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CountedCallLoop {
    pub(super) preheader: usize,
    pub(super) head: usize,
    pub(super) backedge: usize,
}

pub(super) fn find(output: &MachineFunction) -> Option<CountedCallLoop> {
    output
        .instructions
        .iter()
        .enumerate()
        .rev()
        .find_map(|(backedge, instruction)| {
            let head = match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target < backedge =>
                {
                    *target
                }
                _ => return None,
            };
            bounds(output, head, backedge)
        })
}

fn bounds(output: &MachineFunction, head: usize, backedge: usize) -> Option<CountedCallLoop> {
    if !output.instructions[head..backedge]
        .iter()
        .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
    {
        return None;
    }
    let step_registers: Vec<u8> = output.instructions[backedge.saturating_sub(8)..backedge]
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::AddImmediate { d, a, immediate }
                if d == a && *immediate > 0 =>
            {
                Some(*d)
            }
            _ => None,
        })
        .collect();
    if step_registers.is_empty() {
        return None;
    }
    let search = head.saturating_sub(10)..head;
    let initializers: Vec<usize> = step_registers
        .iter()
        .map(|register| {
            search.clone().rev().find(|index| {
                matches!(
                    output.instructions[*index],
                    Instruction::AddImmediate {
                        d,
                        a: 0,
                        immediate: 0,
                    } if d == *register
                )
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CountedCallLoop {
        preheader: *initializers.iter().min()?,
        head,
        backedge,
    })
}
