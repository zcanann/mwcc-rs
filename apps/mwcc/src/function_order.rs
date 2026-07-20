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
pub(crate) fn apply_deferred_emission_order(functions: &mut Vec<MachineFunction>) {
    let source_order = std::mem::take(functions);
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

        apply_deferred_emission_order(&mut functions);

        assert_eq!(
            functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["asm_a", "asm_b", "last", "middle", "first"]
        );
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
