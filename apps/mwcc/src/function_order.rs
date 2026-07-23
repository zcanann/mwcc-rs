//! Translation-unit function emission order.
//!
//! Lowering produces functions in source order. Whole-unit optimizer modes may
//! change the order in which MWCC emits those already-lowered bodies; keeping
//! that transform here prevents driver orchestration and object layout from each
//! accumulating partial versions of the same policy.

use mwcc_machine_code::MachineFunction;
use std::collections::{HashMap, HashSet};

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

/// With inlining disabled, reachable inline definitions materialize when the
/// compiler first reaches a call to them. Emit each recovered definition after
/// that first caller, then recursively emit its own recovered callees. This is
/// distinct from source order and from deferred-inlining reversal.
pub(crate) fn interleave_disabled_inline_materializations(
    functions: &mut Vec<MachineFunction>,
    materialized_names: &HashSet<String>,
) {
    if materialized_names.is_empty() {
        return;
    }
    let mut pending = HashMap::new();
    let mut pending_order = Vec::new();
    let mut roots = Vec::new();
    for function in std::mem::take(functions) {
        if materialized_names.contains(&function.name) {
            pending_order.push(function.name.clone());
            pending.insert(function.name.clone(), function);
        } else {
            roots.push(function);
        }
    }

    let mut ordered = Vec::with_capacity(roots.len() + pending.len());
    for root in roots {
        emit_with_materialized_callees(root, &mut pending, &mut ordered);
    }
    // Address-taken definitions can be rooted by data rather than a code
    // relocation. Preserve their recovered-definition order at the tail.
    for name in pending_order {
        if let Some(function) = pending.remove(&name) {
            emit_with_materialized_callees(function, &mut pending, &mut ordered);
        }
    }
    *functions = ordered;
}

fn emit_with_materialized_callees(
    function: MachineFunction,
    pending: &mut HashMap<String, MachineFunction>,
    ordered: &mut Vec<MachineFunction>,
) {
    let callees: Vec<String> = function
        .relocations
        .iter()
        .filter_map(|relocation| match &relocation.target {
            mwcc_machine_code::RelocationTarget::External(target) if pending.contains_key(target) => {
                Some(target.clone())
            }
            _ => None,
        })
        .collect();
    ordered.push(function);
    for callee in callees {
        if let Some(function) = pending.remove(&callee) {
            emit_with_materialized_callees(function, pending, ordered);
        }
    }
}

/// Apply `-inline …,deferred` emission order.
///
/// Hand-written asm is assembled immediately, forming a leading stream in its
/// original relative order. Compiler-generated functions follow in reverse
/// source order. An all-asm translation unit therefore remains unchanged.
pub(crate) fn apply_deferred_emission_order(
    functions: &mut Vec<MachineFunction>,
    source_function_label_bump: u8,
    post_function_label_bump: u8,
) {
    let mut source_order = std::mem::take(functions);

    // Deferred compilation analyzes the compiled bodies in source order before
    // emitting them in reverse. Every compiled body before the eventual emitted
    // head contributes a fixed source-analysis transaction to that head's
    // absolute anonymous ordinal. Once emission starts, each body leaves a much
    // smaller boundary cost behind it; a framed function's two unwind symbols
    // make the observed distance `2 + post_function_label_bump`.
    let compiled_count = source_order
        .iter()
        .filter(|function| !function.is_asm)
        .count();
    let source_transaction_prefix = compiled_count
        .saturating_sub(1)
        .saturating_mul(usize::from(source_function_label_bump)) as u32;
    for function in source_order.iter_mut().filter(|function| !function.is_asm) {
        function
            .post_function_anonymous_bump
            .get_or_insert(post_function_label_bump);
    }

    // Some transactions complete ordinal analysis in source order even though
    // their code and pools emit in reverse order. Transfer that measured work
    // only when a later compiled body actually becomes the reversed head; a
    // one-function unit keeps its ordinary numbering.
    let deferred_source_prefix: u32 = source_order
        .iter()
        .enumerate()
        .filter(|(index, function)| {
            function.deferred_source_prefix_bump != 0
                && source_order[index + 1..].iter().any(|later| !later.is_asm)
        })
        .map(|(_, function)| function.deferred_source_prefix_bump)
        .sum();

    let (mut immediate_asm, mut deferred_compiled): (Vec<_>, Vec<_>) = source_order
        .into_iter()
        .partition(|function| function.is_asm);
    deferred_compiled.reverse();
    if let Some(head) = deferred_compiled.first_mut() {
        // A capture-owned source prefix is the complete transaction for its
        // source body. Adding the generic per-body transaction as well double
        // counts that analysis (the long-long wait capture exposes this).
        let generic_source_prefix = if deferred_source_prefix == 0 {
            source_transaction_prefix
        } else {
            0
        };
        head.anonymous_label_bump += generic_source_prefix + deferred_source_prefix;
        if deferred_source_prefix != 0 {
            // The source-order analysis already bridges these reversed bodies;
            // MWCC does not insert its ordinary compiled-function gap again.
            head.post_function_anonymous_bump = Some(0);
        }
    }
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

        apply_deferred_emission_order(&mut functions, 3, 1);

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

        apply_deferred_emission_order(&mut functions, 3, 1);

        assert_eq!(functions[0].name, "owner");
        assert_eq!(functions[0].anonymous_label_bump, 3);
        assert_eq!(functions[0].post_function_anonymous_bump, Some(1));
        assert_eq!(functions[1].name, "leaf");
        assert_eq!(functions[1].anonymous_label_bump, 0);
    }

    #[test]
    fn reverse_stream_carries_source_transactions_and_small_boundaries() {
        let mut functions = vec![
            function("f1", false),
            function("f2", false),
            function("f3", false),
            function("f4", false),
        ];

        apply_deferred_emission_order(&mut functions, 3, 1);

        assert_eq!(
            functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["f4", "f3", "f2", "f1"]
        );
        assert_eq!(functions[0].anonymous_label_bump, 9);
        assert!(functions
            .iter()
            .all(|function| function.post_function_anonymous_bump == Some(1)));
    }

    #[test]
    fn source_ordinal_work_prefixes_a_later_reversed_head() {
        let mut source_first = function("source_first", false);
        source_first.deferred_source_prefix_bump = 9;
        source_first.anonymous_label_bump = 7;
        let later = function("later", false);
        let mut functions = vec![source_first, later];

        apply_deferred_emission_order(&mut functions, 3, 1);

        assert_eq!(functions[0].name, "later");
        assert_eq!(functions[0].anonymous_label_bump, 9);
        assert_eq!(functions[0].post_function_anonymous_bump, Some(0));
        assert_eq!(functions[1].anonymous_label_bump, 7);
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

    #[test]
    fn disabled_inline_definitions_follow_their_first_caller_recursively() {
        fn calling(name: &str, target: &str) -> MachineFunction {
            let mut function = function(name, false);
            function.relocations.push(mwcc_machine_code::Relocation {
                instruction_index: 0,
                kind: mwcc_machine_code::RelocationKind::Rel24,
                target: mwcc_machine_code::RelocationTarget::External(target.to_string()),
            });
            function
        }

        let mut functions = vec![
            function("before", false),
            calling("caller", "inline_outer"),
            function("after", false),
            calling("inline_outer", "inline_inner"),
            function("inline_inner", false),
        ];
        let materialized = ["inline_outer".to_string(), "inline_inner".to_string()]
            .into_iter()
            .collect();

        interleave_disabled_inline_materializations(&mut functions, &materialized);

        assert_eq!(
            functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["before", "caller", "inline_outer", "inline_inner", "after"]
        );
    }
}
