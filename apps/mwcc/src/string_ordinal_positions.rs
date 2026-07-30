//! Source-positioned accounting for newly introduced function string pools.
//!
//! A later function can reuse strings interned by an earlier body without
//! repeating the optimizer's pool-front analysis transaction. Keep that
//! unit-wide provenance out of per-function lowering.

use mwcc_machine_code::MachineFunction;
use std::collections::HashSet;

pub(crate) fn apply_multiple_new_string_residue(functions: &mut [MachineFunction], residue: u8) {
    if residue == 0 {
        return;
    }

    let mut seen = HashSet::new();
    for function in functions {
        let newly_interned = function
            .string_literals
            .iter()
            .filter(|literal| seen.insert((*literal).clone()))
            .count();
        let direct_calls = function
            .relocations
            .iter()
            .filter(|relocation| {
                relocation.kind == mwcc_machine_code::RelocationKind::Rel24
            })
            .count();
        if newly_interned > 1 && direct_calls > 1 {
            function.anonymous_label_bump += u32::from(residue);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charges_only_the_function_that_introduces_multiple_strings() {
        let call = |index, name: &str| mwcc_machine_code::Relocation {
            instruction_index: index,
            kind: mwcc_machine_code::RelocationKind::Rel24,
            target: mwcc_machine_code::RelocationTarget::External(name.to_owned()),
        };
        let mut first = MachineFunction::new("first");
        first.string_literals = vec![b"file.c".to_vec(), b"message".to_vec()];
        first.relocations = vec![call(0, "report"), call(1, "continue_work")];
        let mut second = MachineFunction::new("second");
        second.string_literals = vec![b"file.c".to_vec(), b"message".to_vec()];
        second.relocations = vec![call(0, "report"), call(1, "continue_work")];
        let mut third = MachineFunction::new("third");
        third.string_literals = vec![b"file.c".to_vec(), b"new".to_vec()];
        third.relocations = vec![call(0, "report"), call(1, "continue_work")];
        let mut single_call = MachineFunction::new("single_call");
        single_call.string_literals = vec![b"new-a".to_vec(), b"new-b".to_vec()];
        single_call.relocations = vec![call(0, "report")];
        let mut functions = vec![first, second, third, single_call];

        apply_multiple_new_string_residue(&mut functions, 1);

        assert_eq!(functions[0].anonymous_label_bump, 1);
        assert_eq!(functions[1].anonymous_label_bump, 0);
        assert_eq!(functions[2].anonymous_label_bump, 0);
        assert_eq!(functions[3].anonymous_label_bump, 0);
    }
}
