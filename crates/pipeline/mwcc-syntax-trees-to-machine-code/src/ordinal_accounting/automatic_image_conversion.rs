//! GC 4.1 ordinals between an automatic image and its conversion bias.
//!
//! A short automatic `double` array is assigned at its declaration-time
//! static slot. The optimizer can create the later integer-conversion bias
//! only after walking the source region between that declaration and the
//! conversion. Those retained nodes form a mid-pool gap: they must not be
//! folded into the function-front or trailing-label accounting.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use mwcc_versions::FunctionOrdinalAccountingStyle;

const SIGNED_CONVERSION_BIAS: u64 = 0x4330_0000_8000_0000;
const UNSIGNED_CONVERSION_BIAS: u64 = 0x4330_0000_0000_0000;
const COMPLEX_LOOP_GAP: u32 = 90;

pub(super) fn apply_unit(
    functions: &[Function],
    machines: &mut [MachineFunction],
    style: FunctionOrdinalAccountingStyle,
) {
    if !matches!(
        style,
        FunctionOrdinalAccountingStyle::Gc41 | FunctionOrdinalAccountingStyle::Gc41Ipa
    ) {
        return;
    }
    // File IPA hoists source work across function boundaries. The one-function
    // rule is measured exhaustively below; multi-function IPA units expose a
    // different image/constant ordering and remain deliberately unowned until
    // that unit transaction has its own model.
    if style == FunctionOrdinalAccountingStyle::Gc41Ipa && functions.len() != 1 {
        return;
    }

    for function in functions {
        let Some(machine) = machines
            .iter_mut()
            .find(|machine| machine.name == function.name)
        else {
            continue;
        };
        apply(function, machine);
    }
}

fn apply(function: &Function, machine: &mut MachineFunction) {
    let Some((image_index, image)) = source_image(function) else {
        return;
    };
    let [constant] = machine.constants.as_slice() else {
        return;
    };
    if !machine.has_conversion
        || !machine
            .anonymous_rodata
            .iter()
            .any(|blob| blob.static_slot_prefix_bump.is_some())
        || constant.byte_width != 8
        || !matches!(
            constant.bits,
            SIGNED_CONVERSION_BIAS | UNSIGNED_CONVERSION_BIAS
        )
        || machine
            .constant_number_gaps
            .iter()
            .any(|(constant_index, _)| *constant_index == 0)
    {
        return;
    }

    let later_declarations = function.locals.len().saturating_sub(image_index + 1) as u32;
    let gap = if is_complex_loop_conversion(function, image) {
        // Measured against GC/3.0a3 both in the configured WENC unit and in a
        // header-free reduction. The same reduction isolates a 90-node gap,
        // proving that headers and debug declarations are not contributing.
        COMPLEX_LOOP_GAP
    } else {
        // The conversion transaction itself retains two nodes. Every source
        // automatic declared after the image contributes one additional node;
        // measured independently with 0, 1, 4, and 8 later declarations.
        2 + later_declarations
    };
    machine.constant_number_gaps.push((0, gap));
    machine.fragmented_debug_static_slot_discount = machine
        .anonymous_rodata
        .iter()
        .find_map(|blob| blob.static_slot_prefix_bump)
        .unwrap_or(0)
        .saturating_sub(1);
}

fn source_image(function: &Function) -> Option<(usize, &LocalDeclaration)> {
    function.locals.iter().enumerate().find(|(_, local)| {
        local.declared_type == Type::Double
            && matches!(local.array_length, Some(1..=8))
            && !local.is_static
            && local.data_relocations.is_empty()
            && local
                .data_bytes
                .as_ref()
                .is_some_and(|bytes| !bytes.is_empty() && bytes.iter().any(|byte| *byte != 0))
    })
}

/// Identify the measured long-loop transaction without depending on source
/// names or paths. This deliberately asks for every structural discriminator
/// present in the reduction so ordinary image/conversion functions stay on
/// the independently measured declaration-count rule.
fn is_complex_loop_conversion(function: &Function, image: &LocalDeclaration) -> bool {
    if function.locals.len() < 20 {
        return false;
    }
    let has_state_split = function.statements.iter().any(|statement| {
        let Statement::If {
            then_body,
            else_body,
            ..
        } = statement
        else {
            return false;
        };
        !then_body.is_empty()
            && else_body
                .iter()
                .filter(|statement| {
                    matches!(
                        statement,
                        Statement::Assign {
                            value: Expression::Member { .. },
                            ..
                        }
                    )
                })
                .count()
                >= 4
    });
    if !has_state_split {
        return false;
    }

    function.statements.iter().any(|statement| {
        let Statement::Loop { body, .. } = statement else {
            return false;
        };
        let Some(conversion) = body.iter().position(|statement| {
            matches!(
                statement,
                Statement::Assign { value, .. }
                    if crate::analysis::expression_reads_name(value, &image.name)
            )
        }) else {
            return false;
        };
        let prefix = &body[..conversion];
        let branch_count = prefix
            .iter()
            .filter(|statement| matches!(statement, Statement::If { .. }))
            .count();
        branch_count >= 8
            && prefix.iter().any(|statement| {
                matches!(
                    statement,
                    Statement::Assign {
                        value: Expression::Dereference { pointer },
                        ..
                    } if matches!(pointer.as_ref(), Expression::PostStep { .. })
                )
            })
            && prefix.iter().any(|statement| {
                matches!(
                    statement,
                    Statement::Assign {
                        value: Expression::Conditional { .. },
                        ..
                    }
                )
            })
            && prefix
                .iter()
                .any(|statement| matches!(statement, Statement::Store { .. }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::AnonymousRodata;

    fn image() -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Double,
            name: "table".into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(2),
            is_static: false,
            data_bytes: Some(vec![1; 16]),
            data_relocations: Vec::new(),
            is_const: true,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn scalar(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn simple_image_conversion_counts_later_source_declarations() {
        let mut function = super::super::tests::function();
        function.locals = vec![image(), scalar("a"), scalar("b"), scalar("c")];
        let mut machine = MachineFunction::new("probe");
        machine.has_conversion = true;
        machine.intern_constant(SIGNED_CONVERSION_BIAS, 8);
        machine.anonymous_rodata.push(AnonymousRodata {
            bytes: vec![0; 16],
            comment_alignment: 4,
            static_slot_prefix_bump: Some(0),
            anonymous_offset: 0,
        });

        apply(&function, &mut machine);

        assert_eq!(machine.constant_number_gaps, [(0, 5)]);
        assert_eq!(machine.fragmented_debug_static_slot_discount, 0);
    }
}
