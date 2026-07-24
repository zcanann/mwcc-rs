//! Relocation equivalence for instruction-scheduling owners.
//!
//! Two loads can share a value when their relocation kinds and targets agree.
//! Constant-pool indices may differ while naming byte-identical constants, so
//! compare the pool entries as a second, semantic equality path.

use mwcc_machine_code::{PoolConstant, Relocation, RelocationTarget};

pub(super) fn same_relocated_value(
    relocations: &[Relocation],
    constants: &[PoolConstant],
    first_index: usize,
    second_index: usize,
) -> bool {
    let first = relocations
        .iter()
        .find(|relocation| relocation.instruction_index == first_index);
    let second = relocations
        .iter()
        .find(|relocation| relocation.instruction_index == second_index);
    matches!((first, second), (Some(first), Some(second))
        if first.kind == second.kind
            && (same_relocation_target(&first.target, &second.target)
                || same_constant_target(&first.target, &second.target, constants)))
}

pub(super) fn same_target_value(
    relocations: &[Relocation],
    constants: &[PoolConstant],
    first_index: usize,
    second_index: usize,
) -> bool {
    let first = relocations
        .iter()
        .find(|relocation| relocation.instruction_index == first_index);
    let second = relocations
        .iter()
        .find(|relocation| relocation.instruction_index == second_index);
    matches!((first, second), (Some(first), Some(second))
        if same_relocation_target(&first.target, &second.target)
            || same_constant_target(&first.target, &second.target, constants))
}

fn same_constant_target(
    left: &RelocationTarget,
    right: &RelocationTarget,
    constants: &[PoolConstant],
) -> bool {
    match (left, right) {
        (RelocationTarget::Constant(left), RelocationTarget::Constant(right)) => {
            constants.get(*left) == constants.get(*right)
        }
        (
            RelocationTarget::ConstantWithAddend(left, left_addend),
            RelocationTarget::ConstantWithAddend(right, right_addend),
        ) => left_addend == right_addend && constants.get(*left) == constants.get(*right),
        _ => false,
    }
}

fn same_relocation_target(left: &RelocationTarget, right: &RelocationTarget) -> bool {
    match (left, right) {
        (RelocationTarget::External(left), RelocationTarget::External(right)) => left == right,
        (
            RelocationTarget::ExternalWithAddend(left, left_addend),
            RelocationTarget::ExternalWithAddend(right, right_addend),
        ) => left == right && left_addend == right_addend,
        (RelocationTarget::Constant(left), RelocationTarget::Constant(right)) => left == right,
        (
            RelocationTarget::ConstantWithAddend(left, left_addend),
            RelocationTarget::ConstantWithAddend(right, right_addend),
        ) => left == right && left_addend == right_addend,
        (RelocationTarget::JumpTable, RelocationTarget::JumpTable)
        | (RelocationTarget::AnonymousRodata, RelocationTarget::AnonymousRodata) => true,
        (RelocationTarget::JumpTableAt(left), RelocationTarget::JumpTableAt(right))
        | (
            RelocationTarget::AnonymousRodataAt(left),
            RelocationTarget::AnonymousRodataAt(right),
        ) => left == right,
        _ => false,
    }
}
