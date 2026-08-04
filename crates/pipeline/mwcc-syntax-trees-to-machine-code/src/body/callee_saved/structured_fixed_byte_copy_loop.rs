//! CTR lowering for a small fixed-count byte-member copy.
//!
//! Optimized build 163 uses CTR even for a three-byte member-array copy. It
//! keeps the logical index in r5, forms source and destination byte indices,
//! and uses indexed loads/stores so the two bases remain available throughout
//! the compact loop body.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn lower_structured_fixed_byte_copy_loop(&mut self) -> bool {
        let Some(plan) = plan(&self.output.instructions) else {
            return false;
        };
        self.prefer_virtual_general(plan.index, 5);
        let value = self.fresh_virtual_general_preferring(3);

        crate::insert_instruction_retargeting(
            self,
            plan.start,
            Instruction::load_immediate(GENERAL_SCRATCH, plan.count),
        );
        crate::insert_instruction_retargeting(
            self,
            plan.start + 2,
            Instruction::MoveToCountRegister { s: GENERAL_SCRATCH },
        );
        crate::remove_instruction_retargeting_to_next(self, plan.start + 9);

        let start = plan.start;
        let old = self.output.instructions[start..start + 10].to_vec();
        let source_base = match old[3] {
            Instruction::LoadWord { d, .. } => d,
            _ => unreachable!("fixed byte-copy source base changed after recognition"),
        };
        let source_index = match old[4] {
            Instruction::Add { d, .. } => d,
            _ => unreachable!("fixed byte-copy source index changed after recognition"),
        };
        let destination_index = match old[6] {
            Instruction::Add { d, .. } => d,
            _ => unreachable!("fixed byte-copy destination index changed after recognition"),
        };
        self.prefer_virtual_general(source_base, 4);
        self.prefer_virtual_general(source_index, 3);
        self.prefer_virtual_general(destination_index, GENERAL_SCRATCH);

        let mut scheduled = old.clone();
        scheduled[4] = Instruction::AddImmediate {
            d: source_index,
            a: plan.index,
            immediate: plan.source_offset,
        };
        scheduled[5] = Instruction::AddImmediate {
            d: destination_index,
            a: plan.index,
            immediate: plan.destination_offset,
        };
        scheduled[6] = old[8].clone();
        scheduled[7] = Instruction::LoadByteZeroIndexed {
            d: value,
            a: source_base,
            b: source_index,
        };
        scheduled[8] = Instruction::StoreByteIndexed {
            s: value,
            a: plan.owner,
            b: destination_index,
        };
        scheduled[9] = Instruction::BranchConditionalForward {
            options: 16,
            condition_bit: 0,
            target: start + 3,
        };
        self.output.instructions[start..start + 10].clone_from_slice(&scheduled);

        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        permutation[start + 5] = start + 7;
        permutation[start + 6] = start + 5;
        permutation[start + 7] = start + 8;
        permutation[start + 8] = start + 6;
        crate::remap_instruction_indices(self, &permutation);
        if let Some(prelude) = copy_prelude_start(&self.output.instructions, start) {
            schedule_setup_before_stores(self, prelude);
        }
        true
    }
}

fn copy_prelude_start(instructions: &[Instruction], setup: usize) -> Option<usize> {
    let start = setup.checked_sub(14)?;
    let window = instructions.get(start..setup + 3)?;
    if !matches!(window[0], Instruction::LoadFloatSingle { .. })
        || !matches!(window[1], Instruction::StoreFloatSingle { .. })
        || !(2..14).step_by(3).all(|index| {
            matches!(window[index], Instruction::LoadWord { .. })
                && matches!(window[index + 1], Instruction::LoadFloatSingle { .. })
                && matches!(window[index + 2], Instruction::StoreFloatSingle { .. })
        })
        || !matches!(window[14], Instruction::AddImmediate { a: 0, immediate, .. } if immediate > 0)
        || !matches!(window[15], Instruction::AddImmediate { a: 0, immediate: 0, .. })
        || !matches!(window[16], Instruction::MoveToCountRegister { .. })
    {
        return None;
    }
    Some(start)
}

fn schedule_setup_before_stores(generator: &mut Generator, start: usize) {
    let old = generator.output.instructions[start..start + 17].to_vec();
    let mut scheduled = Vec::with_capacity(17);
    scheduled.push(old[0].clone());
    scheduled.push(old[14].clone());
    scheduled.push(old[15].clone());
    scheduled.extend_from_slice(&old[1..14]);
    scheduled.push(old[16].clone());
    generator.output.instructions[start..start + 17].clone_from_slice(&scheduled);

    let mut permutation: Vec<usize> = (0..generator.output.instructions.len()).collect();
    permutation[start + 14] = start + 1;
    permutation[start + 15] = start + 2;
    for relative in 1..14 {
        permutation[start + relative] = start + relative + 2;
    }
    crate::remap_instruction_indices(generator, &permutation);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    start: usize,
    owner: u8,
    index: u8,
    count: i16,
    source_offset: i16,
    destination_offset: i16,
}

fn plan(instructions: &[Instruction]) -> Option<Plan> {
    instructions.windows(9).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediate { d: index, a: 0, immediate: 0 },
            Instruction::LoadWord { d: source_base, a: owner, .. },
            Instruction::Add { d: source_address, a: added_source, b: source_index },
            Instruction::LoadByteZero { d: value, a: loaded_source, offset: source_offset },
            Instruction::Add { d: destination_address, a: destination_owner, b: destination_index },
            Instruction::StoreByte { s: stored, a: stored_destination, offset: destination_offset },
            Instruction::AddImmediate { d: incremented, a: increment_source, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: compared, immediate: count },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target },
        ] = window
        else {
            return None;
        };
        (*added_source == *source_base
            && *source_index == *index
            && *loaded_source == *source_address
            && *destination_owner == *owner
            && *destination_index == *index
            && *stored_destination == *destination_address
            && *stored == *value
            && *incremented == *index
            && *increment_source == *index
            && *compared == *index
            && *target == start + 1
            && (1..=8).contains(count)
            && *source_offset >= 0
            && *destination_offset >= 0)
            .then_some(Plan {
                start,
                owner: *owner,
                index: *index,
                count: i16::try_from(*count).ok()?,
                source_offset: *source_offset,
                destination_offset: *destination_offset,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_fixed_member_byte_copy() {
        let instructions = vec![
            Instruction::AddImmediate { d: 39, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 44, a: 32, offset: 4 },
            Instruction::Add { d: 45, a: 44, b: 39 },
            Instruction::LoadByteZero { d: 0, a: 45, offset: 98 },
            Instruction::Add { d: 46, a: 32, b: 39 },
            Instruction::StoreByte { s: 0, a: 46, offset: 184 },
            Instruction::AddImmediate { d: 39, a: 39, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: 39, immediate: 3 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 1 },
        ];
        assert_eq!(
            plan(&instructions),
            Some(Plan {
                start: 0,
                owner: 32,
                index: 39,
                count: 3,
                source_offset: 98,
                destination_offset: 184,
            })
        );
    }
}
