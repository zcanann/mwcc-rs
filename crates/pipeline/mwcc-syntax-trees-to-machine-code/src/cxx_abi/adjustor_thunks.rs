//! Unit-level emission of CodeWarrior C++ `this`-adjustor thunks.
//!
//! Secondary vtable components reference symbols spelled
//! `@<offset>@<destructor>`. They are real weak functions, not aliases: each
//! subtracts the component offset from r3 and sibling-branches to the complete
//! destructor. Vtable relocation targets are the authoritative demand set.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{
    Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::GlobalDeclaration;
use std::collections::HashSet;

pub(crate) fn lower_vtable_adjustor_thunks(
    globals: &[GlobalDeclaration],
) -> Compilation<Vec<MachineFunction>> {
    let mut groups: Vec<(String, Vec<(String, u32)>)> = Vec::new();
    let mut seen = HashSet::new();
    for global in globals {
        for (_, symbol, _) in &global.data_relocations {
            let Some((offset, destructor)) = parse_adjustor_symbol(symbol) else {
                continue;
            };
            if !seen.insert(symbol.clone()) {
                continue;
            }
            if let Some((_, thunks)) = groups.iter_mut().find(|(name, _)| name == destructor) {
                thunks.push((symbol.clone(), offset));
            } else {
                groups.push((
                    destructor.to_string(),
                    vec![(symbol.clone(), offset)],
                ));
            }
        }
    }

    let mut output = Vec::new();
    for (destructor, mut thunks) in groups {
        // MWCC emits the first non-primary component first, then unwinds the
        // remaining component requests as a stack. For a four-component group
        // this is the measured 20, 104, 88 adjustment order.
        if thunks.len() > 1 {
            thunks[1..].reverse();
        }
        for (name, offset) in thunks {
            let positive = i16::try_from(offset).map_err(|_| {
                Diagnostic::error(format!(
                    "C++ adjustor thunk offset {offset} exceeds signed addi range"
                ))
            })?;
            let immediate = positive.checked_neg().ok_or_else(|| {
                Diagnostic::error(format!(
                    "C++ adjustor thunk offset {offset} cannot be negated"
                ))
            })?;
            let mut function = MachineFunction::new(name);
            function.instructions = vec![
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate,
                },
                Instruction::BranchExternal {
                    target: destructor.clone(),
                },
            ];
            function.relocations.push(Relocation {
                instruction_index: 1,
                kind: RelocationKind::Rel24,
                target: RelocationTarget::External(destructor.clone()),
            });
            function.symbol_order.push(destructor.clone());
            function
                .referenced_function_symbols
                .push(destructor.clone());
            function.implicit_external_callees.push(destructor.clone());
            function.is_weak = true;
            output.push(function);
        }
    }
    Ok(output)
}

fn parse_adjustor_symbol(symbol: &str) -> Option<(u32, &str)> {
    let (offset, target) = symbol.strip_prefix('@')?.split_once('@')?;
    let offset = offset.parse().ok()?;
    target.starts_with("__dt__").then_some((offset, target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_destructor_adjustor_symbols() {
        assert_eq!(
            parse_adjustor_symbol("@104@__dt__7DerivedFv"),
            Some((104, "__dt__7DerivedFv"))
        );
        assert_eq!(parse_adjustor_symbol("__dt__7DerivedFv"), None);
        assert_eq!(parse_adjustor_symbol("@8@ordinary"), None);
    }
}
