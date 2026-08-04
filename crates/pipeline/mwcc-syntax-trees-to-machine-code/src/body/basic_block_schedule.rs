//! Content-only permutations within an already resolved basic block.
//!
//! Relocations and deferred data patches belong to instruction contents and
//! move through the permutation. Branch destinations, jump-table entries, and
//! labels denote the block boundary and deliberately stay at their indices.

use mwcc_machine_code::MachineFunction;

pub(super) fn permute_contents<const N: usize>(
    output: &mut MachineFunction,
    start: usize,
    order: [usize; N],
) {
    let old = output.instructions[start..start + N].to_vec();
    for (new, old_index) in order.into_iter().enumerate() {
        output.instructions[start + new] = old[old_index].clone();
    }
    let mut old_to_new = [0usize; N];
    for (new, old_index) in order.into_iter().enumerate() {
        old_to_new[old_index] = new;
    }
    let remap_owner = |instruction_index: &mut usize| {
        if (start..start + N).contains(instruction_index) {
            *instruction_index = start + old_to_new[*instruction_index - start];
        }
    };
    for relocation in &mut output.relocations {
        remap_owner(&mut relocation.instruction_index);
    }
    output
        .relocations
        .sort_by_key(|relocation| relocation.instruction_index);
    for displacement in &mut output.data_section_displacements {
        remap_owner(&mut displacement.instruction_index);
    }
}
