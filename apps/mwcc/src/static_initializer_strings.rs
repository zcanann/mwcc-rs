//! Anonymous identities created inside constant static-local initializers.
//!
//! A local claims its `$N` identity, then creates any new literals before its
//! declaration closes. Reused literals do not consume another identity.

use mwcc_machine_code::MachineFunction;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct Plan {
    pub string_numbers: HashMap<usize, u32>,
    pub local_adjustments: HashMap<String, u32>,
}

pub(crate) fn plan(
    function: &MachineFunction,
    counter: u32,
    prior_strings: impl Iterator<Item = Vec<u8>>,
) -> Plan {
    let mut plan = Plan::default();
    // Captures with deferred pools own their separate numbering transactions.
    if function.string_number_adjust != 0
        || function.string_number_after_constants.is_some()
        || function.string_number_after_rodata.is_some()
    {
        return plan;
    }
    let mut seen: HashSet<Vec<u8>> = prior_strings.collect();
    let mut introduced = 0;
    for (local_index, local) in function.static_locals.iter().enumerate() {
        plan.local_adjustments
            .insert(local.name.clone(), introduced);
        for (_, target, _) in &local.relocations {
            let Some(index) = target
                .strip_prefix("@@str")
                .and_then(|s| s.parse::<usize>().ok())
            else {
                continue;
            };
            let Some(bytes) = function.string_literals.get(index) else {
                continue;
            };
            if function.string_literal_symbols.contains_key(&index) || !seen.insert(bytes.clone()) {
                continue;
            }
            plan.string_numbers
                .insert(index, counter + local_index as u32 + introduced);
            introduced += 1;
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::StaticLocal;

    fn local(name: &str, targets: &[&str]) -> StaticLocal {
        StaticLocal {
            name: name.into(),
            initial_bytes: Some(vec![0; 4]),
            size: 4,
            alignment: 4,
            is_const: false,
            relocations: targets
                .iter()
                .enumerate()
                .map(|(i, name)| (i as u32 * 4, (*name).into(), 0))
                .collect(),
        }
    }

    #[test]
    fn declarations_interleave_fresh_literals_and_reuse_prior_storage() {
        let mut function = MachineFunction::new("get");
        function.string_literals = vec![b"one".to_vec(), b"two".to_vec()];
        function.static_locals = vec![
            local("p", &["@@str0"]),
            local("q", &["@@str1", "@@str0"]),
            local("n", &[]),
        ];
        let fresh = plan(&function, 5, std::iter::empty());
        assert_eq!(fresh.string_numbers, HashMap::from([(0, 5), (1, 7)]));
        assert_eq!(
            fresh.local_adjustments,
            HashMap::from([("p".into(), 0), ("q".into(), 1), ("n".into(), 2)])
        );
        let reused = plan(&function, 6, [b"one".to_vec()].into_iter());
        assert_eq!(reused.string_numbers, HashMap::from([(1, 7)]));
        assert_eq!(reused.local_adjustments["q"], 0);
        assert_eq!(reused.local_adjustments["n"], 1);
    }
}
