//! A callee-saved `.data` anchor for several global reads across calls.
//!
//! Under absolute addressing build 163 can materialize the writable data
//! section once and address several source globals by their proven section
//! offsets. The base is a virtual live range, so the ordinary allocator and
//! frame reconciler choose and save its physical register.

use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, PointerElement, Type};
use mwcc_versions::{Behavior, FrameConvention, GlobalAddressing};

use crate::generator::DataSectionAnchorPlan;

use super::structured_expression_visit::visit_statement;

pub(crate) fn plan(
    function: &Function,
    globals: &[GlobalDeclaration],
    behavior: Behavior,
) -> Option<DataSectionAnchorPlan> {
    if behavior.frame_convention != FrameConvention::LinkageFirst {
        return None;
    }
    let offsets = source_ordered_data_offsets(
        globals,
        behavior.global_addressing == GlobalAddressing::SmallData,
    )?;
    let mut referenced = std::collections::HashSet::new();
    for statement in &function.statements {
        visit_statement(statement, &mut |expression| {
            if let Expression::Variable(name) = expression {
                if offsets.contains_key(name) {
                    referenced.insert(name.clone());
                }
            }
        });
    }
    if let Some(expression) = &function.return_expression {
        collect_expression_variables(expression, &offsets, &mut referenced);
    }
    if referenced.len() < 3 {
        return None;
    }
    Some(DataSectionAnchorPlan {
        offsets: referenced
            .into_iter()
            .map(|name| {
                let offset = offsets[&name];
                (name, offset)
            })
            .collect(),
        register: None,
    })
}

fn source_ordered_data_offsets(
    globals: &[GlobalDeclaration],
    small_data: bool,
) -> Option<std::collections::HashMap<String, i16>> {
    let mut cursor = 0u32;
    let mut offsets = std::collections::HashMap::new();
    for global in globals {
        let Some((size, alignment)) = initialized_writable_layout(global, small_data)? else {
            continue;
        };
        cursor = cursor.div_ceil(alignment) * alignment;
        offsets.insert(global.name.clone(), i16::try_from(cursor).ok()?);
        cursor = cursor.checked_add(size)?;
    }
    Some(offsets)
}

fn initialized_writable_layout(
    global: &GlobalDeclaration,
    small_data: bool,
) -> Option<Option<(u32, u32)>> {
    if !global.is_data_definition() || global.is_const {
        return Some(None);
    }
    if global.functions_before != 0
        || global.section.as_deref().is_some_and(|section| section != ".data")
    {
        return None;
    }
    let (element_size, natural_alignment) = match global.declared_type {
        Type::Struct { size, align } => (u32::from(size), u32::from(align)),
        other => {
            let size = u32::from(other.width()) / 8;
            (size, size)
        }
    };
    let count = u32::from(global.array_length.unwrap_or(1));
    let size = element_size.checked_mul(count)?;
    if small_data && size <= 8 {
        return Some(None);
    }
    let alignment = natural_alignment
        .max(if global.array_length.is_some() { 4 } else { 1 })
        .max(u32::from(global.attribute_alignment.unwrap_or(1)));
    let initialized = if let Some(bytes) = &global.data_bytes {
        bytes.iter().any(|byte| *byte != 0)
            || !global.data_relocations.is_empty()
            || global.array_length.is_some()
            || global.name.starts_with("__vt__")
    } else if let Some(values) = &global.initializer {
        values.iter().any(|value| *value != 0) || global.array_length.is_some()
    } else if let Some(elements) = &global.address_initializer {
        elements.iter().any(|element| {
            !matches!(element, PointerElement::Null | PointerElement::Scalar(0))
        })
    } else {
        false
    };
    Some(initialized.then_some((size, alignment)))
}

fn collect_expression_variables(
    expression: &Expression,
    offsets: &std::collections::HashMap<String, i16>,
    referenced: &mut std::collections::HashSet<String>,
) {
    match expression {
        Expression::Variable(name) => {
            if offsets.contains_key(name) {
                referenced.insert(name.clone());
            }
        }
        Expression::Assign { target, value }
        | Expression::Binary {
            left: target,
            right: value,
            ..
        }
        | Expression::Comma {
            left: target,
            right: value,
        }
        | Expression::Index {
            base: target,
            index: value,
        } => {
            collect_expression_variables(target, offsets, referenced);
            collect_expression_variables(value, offsets, referenced);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. }
        | Expression::PostStep {
            target: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_expression_variables(operand, offsets, referenced);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_expression_variables(condition, offsets, referenced);
            collect_expression_variables(when_true, offsets, referenced);
            collect_expression_variables(when_false, offsets, referenced);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                collect_expression_variables(argument, offsets, referenced);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(name: &str, bytes: Vec<u8>) -> GlobalDeclaration {
        GlobalDeclaration {
            declared_type: Type::Struct { size: 12, align: 4 },
            source_fundamental: None,
            name: name.into(),
            is_extern: false,
            is_static: false,
            is_volatile: false,
            is_weak: false,
            force_active: false,
            non_static_functions_before: 0,
            functions_before: 0,
            array_length: None,
            array_length_inferred: false,
            initializer: None,
            is_const: false,
            pointer_pointee_const: false,
            address_initializer: None,
            data_bytes: Some(bytes),
            data_relocations: Vec::new(),
            section: None,
            attribute_alignment: None,
        }
    }

    #[test]
    fn lays_out_source_ordered_initialized_structs() {
        let offsets = source_ordered_data_offsets(
            &[
                global("a", vec![1; 12]),
                global("b", vec![2; 12]),
                global("c", vec![3; 12]),
            ],
            true,
        )
        .unwrap();

        assert_eq!(offsets["a"], 0);
        assert_eq!(offsets["b"], 12);
        assert_eq!(offsets["c"], 24);
    }
}
