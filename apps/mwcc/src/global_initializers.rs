//! Classification of address-bearing global initializers.
//!
//! The driver serializes supported tables, while this module owns the linkage
//! and target-provenance checks that decide when private tables are safe to
//! lower. Keeping those checks separate makes new initializer families
//! measurable without adding more policy to the driver loop.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{GlobalDeclaration, PointerElement, Type};

pub(crate) fn private_unit_function_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
    functions: &[MachineFunction],
) -> bool {
    global.is_static
        && !global.is_const
        && global.array_length.is_some()
        && elements.iter().all(|element| {
            matches!(element, PointerElement::Symbol(name)
                if functions.iter().any(|function| &function.name == name))
                || matches!(element, PointerElement::Null)
        })
}

/// A private aggregate whose address fields all name storage defined by this
/// translation unit has no unresolved symbol-order question: every target gets
/// an object symbol from the same writer pass. Animal Crossing animation data
/// uses this for `{ left_table, right_table, enum_value, NULL }` records.
pub(crate) fn private_unit_data_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
    globals: &[GlobalDeclaration],
) -> bool {
    global.is_static
        && !global.is_const
        && matches!(global.declared_type, Type::Struct { .. })
        && elements.iter().all(|element| match element {
            PointerElement::Symbol(name) => globals
                .iter()
                .any(|candidate| candidate.name == *name && candidate.is_data_definition()),
            PointerElement::Null | PointerElement::Scalar(_) => true,
            PointerElement::Str(_) => false,
        })
}
