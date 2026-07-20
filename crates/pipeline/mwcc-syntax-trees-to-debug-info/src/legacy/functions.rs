//! Function and formal-parameter DIEs for monolithic legacy DWARF-1 units.

use super::{attribute, data, FUNCTION_END};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, AttributeName, AttributeValue, DebugEntry, DebugEntryId, DebugRecord, Tag,
};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{Function, TranslationUnit, Type};
use std::collections::HashMap;

struct FunctionPlan<'a> {
    function: &'a Function,
    function_id: DebugEntryId,
    parameter_ids: Vec<DebugEntryId>,
    parameter_end: Option<DebugEntryId>,
}

pub(super) fn records<'a>(
    unit: &'a TranslationUnit,
    functions: &[&'a Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
    aggregate_ids: &HashMap<String, DebugEntryId>,
) -> Compilation<Vec<DebugRecord>> {
    let mut next_id = first_id.0;
    let mut plans = Vec::with_capacity(functions.len());
    for function in functions {
        let function_id = allocate(&mut next_id);
        let parameter_ids = function
            .parameters
            .iter()
            .map(|_| allocate(&mut next_id))
            .collect::<Vec<_>>();
        let parameter_end = (!parameter_ids.is_empty()).then(|| allocate(&mut next_id));
        plans.push(FunctionPlan {
            function,
            function_id,
            parameter_ids,
            parameter_end,
        });
    }

    let mut records = Vec::new();
    for (index, plan) in plans.iter().enumerate() {
        let sibling = plans
            .get(index + 1)
            .map_or(FUNCTION_END, |following| following.function_id);
        let mut attributes = vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(
                AttributeName::Name,
                AttributeValue::String(plan.function.name.clone()),
            ),
        ];
        if plan.function.return_type != Type::Void {
            attributes.push(data::member_type_attribute(
                plan.function.return_type,
                None,
                None,
            )?);
        }
        attributes.extend([
            attribute(
                AttributeName::LowPc,
                AttributeValue::Address(Address::external(&plan.function.name)),
            ),
            attribute(
                AttributeName::HighPc,
                AttributeValue::Address(Address::external_with_addend(
                    ".text",
                    (layout.offsets[index] + layout.sizes[index]) as i32,
                )),
            ),
        ]);
        records.push(DebugRecord::Entry(DebugEntry {
            id: plan.function_id,
            tag: Tag::GlobalSubroutine,
            attributes,
        }));

        for (parameter_index, parameter) in plan.function.parameters.iter().enumerate() {
            let sibling = plan
                .parameter_ids
                .get(parameter_index + 1)
                .copied()
                .or(plan.parameter_end)
                .expect("a planned parameter list has an end marker");
            let aggregate_id = unit
                .function_parameter_aggregate_tags
                .get(&(plan.function.name.clone(), parameter.name.clone()))
                .map(|tag| {
                    aggregate_ids.get(tag).copied().ok_or_else(|| {
                        Diagnostic::error(format!(
                            "debug-info: parameter '{}.{}' references aggregate '{}' without an emitted type DIE",
                            plan.function.name, parameter.name, tag
                        ))
                    })
                })
                .transpose()?;
            let register = u8::try_from(3 + parameter_index).map_err(|_| {
                Diagnostic::error("debug-info: too many integer-class formal parameters")
            })?;
            records.push(DebugRecord::Entry(DebugEntry {
                id: plan.parameter_ids[parameter_index],
                tag: Tag::FormalParameter,
                attributes: vec![
                    attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                    attribute(
                        AttributeName::Name,
                        AttributeValue::String(parameter.name.clone()),
                    ),
                    data::member_type_attribute(parameter.parameter_type, aggregate_id, None)?,
                    attribute(
                        AttributeName::Location,
                        AttributeValue::Block2(vec![1, 0, 0, 0, register]),
                    ),
                ],
            }));
        }
        if let Some(end) = plan.parameter_end {
            records.push(DebugRecord::Marker(end));
            records.push(DebugRecord::Raw(vec![0, 0, 0, 4]));
        }
    }
    records.extend([
        DebugRecord::Marker(FUNCTION_END),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
        DebugRecord::Raw(vec![0, 0, 0, 4]),
    ]);
    Ok(records)
}

fn allocate(next_id: &mut u32) -> DebugEntryId {
    let id = DebugEntryId(*next_id);
    *next_id += 1;
    id
}
