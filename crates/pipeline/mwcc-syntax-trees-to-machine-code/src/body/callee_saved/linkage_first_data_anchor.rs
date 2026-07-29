//! A callee-saved `.data` anchor for several global reads across calls.
//!
//! Under absolute addressing build 163 can materialize the writable data
//! section once and address several source globals by their proven section
//! offsets. The base is a virtual live range, so the ordinary allocator and
//! frame reconciler choose and save its physical register.

use mwcc_syntax_trees::{
    Expression, Function, GlobalDeclaration, PointerElement, Statement, Type,
};
use mwcc_versions::{Behavior, FrameConvention, GlobalAddressing};

use crate::generator::DataSectionAnchorPlan;
use crate::InlineBodySet;

use super::structured_expression_visit::{visit_expression, visit_statement};
use super::structured_locals::DeferredSavedHomePlan;

pub(crate) fn plan(
    function: &Function,
    globals: &[GlobalDeclaration],
    behavior: Behavior,
    inline_bodies: &InlineBodySet,
) -> Option<DataSectionAnchorPlan> {
    if behavior.frame_convention != FrameConvention::LinkageFirst {
        return None;
    }
    // Section-base selection happens before body lowering, while automatic
    // inline composition happens during it. Analyze the same effective body
    // here so globals referenced only by a retained helper still participate
    // in the caller's shared `.data` anchor.
    let expanded;
    let function = if let Some(body) = inline_bodies.expand_calls(function) {
        expanded = body;
        &expanded
    } else {
        function
    };
    let offsets = source_ordered_data_offsets(
        globals,
        behavior.global_addressing == GlobalAddressing::SmallData,
        behavior.inferred_array_uses_full_data_section,
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

/// Find a deferred saved-home whose first value starts after the anchor's last
/// source use. MWCC can use that home for the section base first, then redefine
/// it with the later call result without growing the saved-register range.
pub(super) fn reusable_deferred_group(
    function: &Function,
    anchor: &DataSectionAnchorPlan,
    deferred: &DeferredSavedHomePlan,
) -> Option<usize> {
    let mut cursor = 0usize;
    let mut last_reference = None;
    collect_last_reference_position(
        &function.statements,
        &anchor.offsets,
        &mut cursor,
        &mut last_reference,
    )?;
    let last_reference = last_reference?;
    (0..deferred.group_count)
        .filter(|group| deferred.first_assignment(*group) > last_reference)
        .min_by_key(|group| deferred.first_assignment(*group))
}

fn collect_last_reference_position(
    statements: &[Statement],
    offsets: &std::collections::HashMap<String, i16>,
    cursor: &mut usize,
    last_reference: &mut Option<usize>,
) -> Option<()> {
    for statement in statements {
        *cursor += 1;
        let position = *cursor;
        let mut references_anchor = false;
        let mut inspect = |expression: &Expression| {
            visit_expression(expression, &mut |nested| {
                if matches!(nested, Expression::Variable(name) if offsets.contains_key(name)) {
                    references_anchor = true;
                }
            });
        };
        match statement {
            Statement::Store { target, value } => {
                inspect(target);
                inspect(value);
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => inspect(value),
            Statement::If { condition, .. } => inspect(condition),
            Statement::Loop {
                initializer,
                condition,
                step,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    inspect(expression);
                }
            }
            Statement::Switch { .. } => return None,
            Statement::Return(None)
            | Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => {}
        }
        if references_anchor {
            *last_reference = Some(position);
        }
        match statement {
            Statement::Loop { body, .. } => {
                collect_last_reference_position(body, offsets, cursor, last_reference)?;
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_last_reference_position(then_body, offsets, cursor, last_reference)?;
                collect_last_reference_position(else_body, offsets, cursor, last_reference)?;
            }
            _ => {}
        }
    }
    Some(())
}

fn source_ordered_data_offsets(
    globals: &[GlobalDeclaration],
    small_data: bool,
    inferred_array_uses_full_data_section: bool,
) -> Option<std::collections::HashMap<String, i16>> {
    let mut cursor = 0u32;
    let mut offsets = std::collections::HashMap::new();
    for global in globals {
        let Some((size, alignment)) = initialized_writable_layout(
            global,
            small_data,
            inferred_array_uses_full_data_section,
        )?
        else {
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
    inferred_array_uses_full_data_section: bool,
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
    let forced_full_data = inferred_array_uses_full_data_section
        && global.array_length_inferred
        && !global.is_static;
    if small_data && size <= 8 && !forced_full_data {
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
    use mwcc_versions::{CompilerConfig, GC_1_2_5N};

    fn function(name: &str, statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: name.into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn call(name: &str, arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments,
        })
    }

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
            true,
        )
        .unwrap();

        assert_eq!(offsets["a"], 0);
        assert_eq!(offsets["b"], 12);
        assert_eq!(offsets["c"], 24);
    }

    #[test]
    fn retains_small_inferred_arrays_for_full_data_layout() {
        let mut inferred = global("inferred", vec![1; 8]);
        inferred.declared_type = Type::Float;
        inferred.array_length = Some(2);
        inferred.array_length_inferred = true;
        let offsets =
            source_ordered_data_offsets(&[inferred.clone(), global("large", vec![2; 12])], true, true)
                .unwrap();

        assert_eq!(offsets["inferred"], 0);
        assert_eq!(offsets["large"], 8);

        let without_full_data =
            source_ordered_data_offsets(&[inferred, global("large", vec![2; 12])], true, false)
                .unwrap();
        assert!(!without_full_data.contains_key("inferred"));
        assert_eq!(without_full_data["large"], 0);
    }

    #[test]
    fn includes_globals_reached_through_automatic_inline_expansion() {
        let helper = function(
            "helper",
            vec![call(
                "consume",
                vec![Expression::Variable("inline_global".into())],
            )],
        );
        let inline_bodies = InlineBodySet::analyze(&[helper]);
        let caller = function(
            "caller",
            vec![
                call(
                    "consume",
                    vec![
                        Expression::Variable("direct_a".into()),
                        Expression::Variable("direct_b".into()),
                    ],
                ),
                call("helper", Vec::new()),
            ],
        );
        let globals = vec![
            global("direct_a", vec![1; 12]),
            global("direct_b", vec![2; 12]),
            global("inline_global", vec![3; 12]),
        ];
        let behavior = Behavior::resolve(&CompilerConfig::new(GC_1_2_5N));

        let anchor = plan(&caller, &globals, behavior, &inline_bodies)
            .expect("the expanded body references three writable data globals");

        assert_eq!(anchor.offsets.len(), 3);
        assert_eq!(anchor.offsets["direct_a"], 0);
        assert_eq!(anchor.offsets["direct_b"], 12);
        assert_eq!(anchor.offsets["inline_global"], 24);
    }
}
