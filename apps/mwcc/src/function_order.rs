//! Translation-unit function emission order.
//!
//! Lowering produces functions in source order. Whole-unit optimizer modes may
//! change the order in which MWCC emits those already-lowered bodies; keeping
//! that transform here prevents driver orchestration and object layout from each
//! accumulating partial versions of the same policy.

use mwcc_machine_code::MachineFunction;

/// Whether every earlier call site consumed a terminal implicitly-materialized
/// inline. A surviving relocation proves the out-of-line copy is still needed.
pub(crate) fn terminal_implicit_inline_is_consumed(
    name: &str,
    lowered_callers: &[MachineFunction],
) -> bool {
    !lowered_callers.iter().any(|function| {
        function.relocations.iter().any(|relocation| {
            matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(target) if target == name
            )
        })
    })
}

/// Apply `-inline …,deferred` emission order.
///
/// Hand-written asm is assembled immediately, forming a leading stream in its
/// original relative order. Compiler-generated functions follow in reverse
/// source order. An all-asm translation unit therefore remains unchanged.
pub(crate) fn apply_deferred_emission_order(
    functions: &mut Vec<MachineFunction>,
    post_leaf_anonymous_bump: u8,
) {
    let mut source_order = std::mem::take(functions);

    // Deferred emission reverses compiled bodies, but a leading source-order
    // run of leaf functions with no anonymous payload was still compiled first.
    // Its post-function bookkeeping therefore advances the ordinal seen by the
    // first later body that owns a pool object or jump table. Carry only this
    // fully characterized prefix; once a function owns anonymous state, a
    // general absolute-ordinal plan is required rather than guessing its cost.
    let mut transparent_prefix = Some(0u32);
    for function in &mut source_order {
        let owns_anonymous_state = function.frame.is_some()
            || function.has_conversion
            || function.has_float_branch
            || function.anonymous_label_bump != 0
            || !function.string_literals.is_empty()
            || !function.constants.is_empty()
            || !function.jump_tables.is_empty()
            || !function.anonymous_rodata.is_empty()
            || !function.static_locals.is_empty();
        if owns_anonymous_state {
            if let Some(prefix) = transparent_prefix {
                function.anonymous_label_bump += prefix;
            }
            transparent_prefix = None;
        } else if let Some(prefix) = &mut transparent_prefix {
            *prefix += u32::from(
                function
                    .post_function_anonymous_bump
                    .unwrap_or(post_leaf_anonymous_bump),
            );
        }
    }
    let (mut immediate_asm, mut deferred_compiled): (Vec<_>, Vec<_>) =
        source_order.into_iter().partition(|function| function.is_asm);
    deferred_compiled.reverse();
    immediate_asm.extend(deferred_compiled);
    *functions = immediate_asm;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str, is_asm: bool) -> MachineFunction {
        let mut function = MachineFunction::new(name.to_string());
        function.is_asm = is_asm;
        function
    }

    #[test]
    fn asm_leads_reversed_compiled_functions() {
        let mut functions = vec![
            function("first", false),
            function("asm_a", true),
            function("middle", false),
            function("asm_b", true),
            function("last", false),
        ];

        apply_deferred_emission_order(&mut functions, 4);

        assert_eq!(
            functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["asm_a", "asm_b", "last", "middle", "first"]
        );
    }

    #[test]
    fn transparent_source_leaf_advances_first_anonymous_owner() {
        let mut owner = function("owner", false);
        owner.string_literals.push(b"owned".to_vec());
        let mut functions = vec![function("leaf", false), owner];

        apply_deferred_emission_order(&mut functions, 4);

        assert_eq!(functions[0].name, "owner");
        assert_eq!(functions[0].anonymous_label_bump, 4);
        assert_eq!(functions[1].name, "leaf");
        assert_eq!(functions[1].anonymous_label_bump, 0);
    }

    #[test]
    fn consumed_terminal_inline_has_no_surviving_call() {
        let callers = vec![function("caller", false)];
        assert!(terminal_implicit_inline_is_consumed("helper", &callers));

        let mut caller = function("caller", false);
        caller.relocations.push(mwcc_machine_code::Relocation {
            instruction_index: 0,
            kind: mwcc_machine_code::RelocationKind::Rel24,
            target: mwcc_machine_code::RelocationTarget::External("helper".to_string()),
        });
        assert!(!terminal_implicit_inline_is_consumed("helper", &[caller]));
    }
}
