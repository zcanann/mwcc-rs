//! General legacy DIE composition in declaration-use order.
//!
//! Source types are materialized immediately before the first function that
//! needs them. Stable declaration IDs survive across function boundaries, so
//! aliases share records while independent array declarations remain distinct.

use super::{data, functions};
use mwcc_core::Compilation;
use mwcc_dwarf1::DebugRecord;
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{Function, TranslationUnit, Type};
use std::collections::{HashMap, HashSet};

pub(super) fn records(
    unit: &TranslationUnit,
    data: data::DataRecords,
    source_functions: &[&Function],
    variables: &[functions::FunctionVariables],
    layout: &FunctionLayout,
) -> Compilation<Vec<DebugRecord>> {
    let mut records = data.records;
    let mut aggregate_ids = data.aggregate_ids;
    let mut row_array_ids = HashMap::new();
    let mut next_id = data.next_id;
    let mut seen_rows = HashSet::new();
    for (index, function) in source_functions.iter().enumerate() {
        let mut aggregate_keys = aggregate_ids.keys().cloned().collect::<Vec<_>>();
        let mut type_requests = Vec::new();
        if let Some(tag) = unit.function_return_aggregate_tags.get(&function.name) {
            if !aggregate_keys.contains(tag) {
                aggregate_keys.push(tag.clone());
                type_requests.push(data::GeneralTypeRequest::Aggregate(tag.clone()));
            }
        }
        for parameter in &function.parameters {
            if let Some(row) = unit
                .function_parameter_row_arrays
                .get(&(function.name.clone(), parameter.name.clone()))
            {
                if seen_rows.insert(row.identity) {
                    type_requests.push(data::GeneralTypeRequest::ParameterRowArray(row.clone()));
                }
            }
            if let Some(tag) = unit
                .function_parameter_aggregate_tags
                .get(&(function.name.clone(), parameter.name.clone()))
            {
                if !aggregate_keys.contains(tag) {
                    aggregate_keys.push(tag.clone());
                    type_requests.push(data::GeneralTypeRequest::Aggregate(tag.clone()));
                }
            }
        }
        for (local_index, local) in function.locals.iter().enumerate() {
            if let Some(length) = local.array_length {
                if !matches!(
                    local.declared_type,
                    Type::Pointer(_) | Type::Struct { .. } | Type::StructPointer { .. }
                ) {
                    type_requests.push(data::GeneralTypeRequest::ScalarLocalArray {
                        function: function.name.clone(),
                        local_index,
                        element_type: local.declared_type,
                        source_fundamental: unit
                            .function_local_fundamentals
                            .get(&(function.name.clone(), local.name.clone()))
                            .copied(),
                        length,
                    });
                }
            }
            if let Some(tag) = unit
                .function_local_aggregate_tags
                .get(&(function.name.clone(), local.name.clone()))
            {
                if !aggregate_keys.contains(tag) {
                    aggregate_keys.push(tag.clone());
                    type_requests.push(data::GeneralTypeRequest::Aggregate(tag.clone()));
                }
            }
        }

        let data = data::general_records_directly_followed(
            unit,
            &[],
            next_id,
            &type_requests,
            &aggregate_ids,
        )?;
        aggregate_ids = data.aggregate_ids;
        row_array_ids.extend(data.parameter_row_array_ids);
        let function_plan = functions::selected_plan_with_variables(
            &[*function],
            data.next_id,
            &variables[index..index + 1],
        )?;
        next_id = function_plan.next_id();
        records.extend(data.records);
        let function_layout = FunctionLayout {
            order: vec![0],
            offsets: vec![layout.offsets[index]],
            sizes: vec![layout.sizes[index]],
            byte_len: layout.byte_len,
        };
        records.extend(function_plan.records_with_array_ids(
            unit,
            &function_layout,
            &aggregate_ids,
            &data.local_array_ids,
            &row_array_ids,
            (index + 1 < source_functions.len()).then_some(next_id),
        )?);
    }
    if source_functions.is_empty() {
        records.extend(
            functions::selected_plan_with_variables(&[], next_id, &[])?.records(
                unit,
                layout,
                &aggregate_ids,
                None,
            )?,
        );
    }
    Ok(records)
}
