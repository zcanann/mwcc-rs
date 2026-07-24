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
    selected_parameters: Vec<(usize, VariableLocation)>,
    parameter_ids: Vec<DebugEntryId>,
    selected_locals: Vec<(usize, VariableLocation)>,
    local_ids: Vec<DebugEntryId>,
    children_end: Option<DebugEntryId>,
    global_references: Vec<DebugEntryId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VariableLocation {
    Register(u8),
    Frame(i32),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct FunctionVariables {
    pub parameters: Vec<(usize, VariableLocation)>,
    pub locals: Vec<(usize, VariableLocation)>,
    pub global_references: Vec<DebugEntryId>,
}

/// Allocated function/parameter identities, independent of the DIEs that may
/// follow them. Mixed GC 4.x units need this boundary before data types can be
/// assigned IDs, while function parameter types may in turn reference those
/// data-owned type DIEs.
pub(super) struct SelectedFunctionPlan<'a> {
    functions: Vec<FunctionPlan<'a>>,
    next_id: DebugEntryId,
}

impl SelectedFunctionPlan<'_> {
    pub(super) fn next_id(&self) -> DebugEntryId {
        self.next_id
    }
}

pub(super) fn records<'a>(
    unit: &'a TranslationUnit,
    functions: &[&'a Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
    aggregate_ids: &HashMap<String, DebugEntryId>,
) -> Compilation<Vec<DebugRecord>> {
    let parameter_registers = functions
        .iter()
        .map(|function| {
            function
                .parameters
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    u8::try_from(3 + index)
                        .map(|register| (index, register))
                        .map_err(|_| {
                            Diagnostic::error(
                                "debug-info: too many integer-class formal parameters",
                            )
                        })
                })
                .collect()
        })
        .collect::<Compilation<Vec<_>>>()?;
    selected_records(
        unit,
        functions,
        layout,
        first_id,
        aggregate_ids,
        &parameter_registers,
    )
}

/// Encode functions after liveness/allocation has selected the parameters that
/// survive into debug information and their physical registers.
pub(super) fn selected_records<'a>(
    unit: &'a TranslationUnit,
    functions: &[&'a Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
    aggregate_ids: &HashMap<String, DebugEntryId>,
    parameter_registers: &[Vec<(usize, u8)>],
) -> Compilation<Vec<DebugRecord>> {
    selected_plan(functions, first_id, parameter_registers)?.records(
        unit,
        layout,
        aggregate_ids,
        None,
    )
}

/// Encode one selected function and append MWCC's vendor reference to the
/// file-scope callback object associated with that function.
pub(super) fn selected_records_with_global_reference<'a>(
    unit: &'a TranslationUnit,
    functions: &[&'a Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
    aggregate_ids: &HashMap<String, DebugEntryId>,
    parameter_registers: &[Vec<(usize, u8)>],
    global_id: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    if functions.len() != 1 {
        return Err(Diagnostic::error(
            "debug-info: a global-referenced function plan must contain one function",
        ));
    }
    let mut records = selected_records(
        unit,
        functions,
        layout,
        first_id,
        aggregate_ids,
        parameter_registers,
    )?;
    let Some(DebugRecord::Entry(function)) = records.first_mut() else {
        return Err(Diagnostic::error(
            "debug-info: a global-referenced function plan has no function DIE",
        ));
    };
    function.attributes.push(attribute(
        AttributeName::MwVtableElement,
        AttributeValue::Reference(global_id),
    ));
    Ok(records)
}

pub(super) fn selected_plan<'a>(
    functions: &[&'a Function],
    first_id: DebugEntryId,
    parameter_registers: &[Vec<(usize, u8)>],
) -> Compilation<SelectedFunctionPlan<'a>> {
    let variables = parameter_registers
        .iter()
        .map(|parameters| FunctionVariables {
            parameters: parameters
                .iter()
                .map(|(index, register)| (*index, VariableLocation::Register(*register)))
                .collect(),
            ..FunctionVariables::default()
        })
        .collect::<Vec<_>>();
    selected_plan_with_variables(functions, first_id, &variables)
}

pub(super) fn selected_plan_with_variables<'a>(
    functions: &[&'a Function],
    first_id: DebugEntryId,
    variables: &[FunctionVariables],
) -> Compilation<SelectedFunctionPlan<'a>> {
    if functions.len() != variables.len() {
        return Err(Diagnostic::error(
            "debug-info: function variable plans are not aligned",
        ));
    }
    let mut next_id = first_id.0;
    let mut plans = Vec::with_capacity(functions.len());
    for (function, selected) in functions.iter().zip(variables) {
        let function_id = allocate(&mut next_id);
        let parameter_ids = selected
            .parameters
            .iter()
            .map(|_| allocate(&mut next_id))
            .collect::<Vec<_>>();
        let local_ids = selected
            .locals
            .iter()
            .map(|_| allocate(&mut next_id))
            .collect::<Vec<_>>();
        let children_end = (!(parameter_ids.is_empty() && local_ids.is_empty()))
            .then(|| allocate(&mut next_id));
        plans.push(FunctionPlan {
            function,
            function_id,
            selected_parameters: selected.parameters.clone(),
            parameter_ids,
            selected_locals: selected.locals.clone(),
            local_ids,
            children_end,
            global_references: selected.global_references.clone(),
        });
    }

    Ok(SelectedFunctionPlan {
        functions: plans,
        next_id: DebugEntryId(next_id),
    })
}

impl SelectedFunctionPlan<'_> {
    pub(super) fn records(
        &self,
        unit: &TranslationUnit,
        layout: &FunctionLayout,
        aggregate_ids: &HashMap<String, DebugEntryId>,
        following: Option<DebugEntryId>,
    ) -> Compilation<Vec<DebugRecord>> {
        let mut records = Vec::new();
        for (index, plan) in self.functions.iter().enumerate() {
            let sibling = self
                .functions
                .get(index + 1)
                .map_or(following.unwrap_or(FUNCTION_END), |following| {
                    following.function_id
                });
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
            attributes.extend(plan.global_references.iter().copied().map(|global_id| {
                attribute(
                    AttributeName::MwVtableElement,
                    AttributeValue::Reference(global_id),
                )
            }));
            records.push(DebugRecord::Entry(DebugEntry {
                id: plan.function_id,
                tag: if plan.function.is_static {
                    Tag::LocalSubroutine
                } else {
                    Tag::GlobalSubroutine
                },
                attributes,
            }));

            for (selected_index, (parameter_index, location)) in
                plan.selected_parameters.iter().copied().enumerate()
            {
                let parameter = plan
                    .function
                    .parameters
                    .get(parameter_index)
                    .ok_or_else(|| {
                        Diagnostic::error("debug-info: selected parameter index is out of range")
                    })?;
                let sibling = plan
                    .parameter_ids
                    .get(selected_index + 1)
                    .copied()
                    .or_else(|| plan.local_ids.first().copied())
                    .or(plan.children_end)
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
                records.push(DebugRecord::Entry(DebugEntry {
                    id: plan.parameter_ids[selected_index],
                    tag: Tag::FormalParameter,
                    attributes: vec![
                        attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                        attribute(
                            AttributeName::Name,
                            AttributeValue::String(parameter.name.clone()),
                        ),
                        data::member_type_attribute(parameter.parameter_type, aggregate_id, None)?,
                        location_attribute(location),
                    ],
                }));
            }
            for (selected_index, (local_index, location)) in
                plan.selected_locals.iter().copied().enumerate()
            {
                let local = plan.function.locals.get(local_index).ok_or_else(|| {
                    Diagnostic::error("debug-info: selected local index is out of range")
                })?;
                let sibling = plan
                    .local_ids
                    .get(selected_index + 1)
                    .copied()
                    .or(plan.children_end)
                    .expect("a planned local list has an end marker");
                let aggregate_id = unit
                    .function_local_aggregate_tags
                    .get(&(plan.function.name.clone(), local.name.clone()))
                    .map(|tag| {
                        aggregate_ids.get(tag).copied().ok_or_else(|| {
                            Diagnostic::error(format!(
                                "debug-info: local '{}.{}' references aggregate '{}' without an emitted type DIE",
                                plan.function.name, local.name, tag
                            ))
                        })
                    })
                    .transpose()?;
                records.push(DebugRecord::Entry(DebugEntry {
                    id: plan.local_ids[selected_index],
                    tag: Tag::LocalVariable,
                    attributes: vec![
                        attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                        attribute(
                            AttributeName::Name,
                            AttributeValue::String(local.name.clone()),
                        ),
                        data::member_type_attribute(local.declared_type, aggregate_id, None)?,
                        location_attribute(location),
                    ],
                }));
            }
            if let Some(end) = plan.children_end {
                records.push(DebugRecord::Marker(end));
                records.push(DebugRecord::Raw(vec![0, 0, 0, 4]));
            }
        }
        if following.is_none() {
            records.extend([
                DebugRecord::Marker(FUNCTION_END),
                DebugRecord::Raw(vec![0, 0, 0, 4]),
                DebugRecord::Raw(vec![0, 0, 0, 4]),
            ]);
        }
        Ok(records)
    }
}

fn location_attribute(location: VariableLocation) -> mwcc_dwarf1::Attribute {
    let bytes = match location {
        VariableLocation::Register(register) => vec![0x01, 0, 0, 0, register],
        VariableLocation::Frame(offset) => {
            let mut bytes = vec![0x02, 0, 0, 0, 1, 0x04];
            bytes.extend_from_slice(&offset.to_be_bytes());
            bytes.push(0x07);
            bytes
        }
    };
    attribute(AttributeName::Location, AttributeValue::Block2(bytes))
}

fn allocate(next_id: &mut u32) -> DebugEntryId {
    let id = DebugEntryId(*next_id);
    *next_id += 1;
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Parameter, Pointee};

    #[test]
    fn selected_plan_exposes_parameter_and_terminator_id_span() {
        let first = function("first");
        let second = function("second");
        let plan = selected_plan(
            &[&first, &second],
            DebugEntryId(1),
            &[Vec::new(), vec![(0, 31)]],
        )
        .unwrap();

        // Function IDs consume 1 and 2; the retained parameter and its child
        // terminator consume 3 and 4. Following DIE families start at 5.
        assert_eq!(plan.next_id(), DebugEntryId(5));
    }

    #[test]
    fn variable_locations_encode_legacy_register_and_frame_expressions() {
        assert_eq!(
            location_attribute(VariableLocation::Register(30)).value,
            AttributeValue::Block2(vec![0x01, 0, 0, 0, 30])
        );
        assert_eq!(
            location_attribute(VariableLocation::Frame(20)).value,
            AttributeValue::Block2(vec![
                0x02, 0, 0, 0, 1, 0x04, 0, 0, 0, 20, 0x07,
            ])
        );
    }

    fn function(name: &str) -> Function {
        Function {
            return_type: Type::Void,
            name: name.into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(Pointee::Int),
                name: "destination".into(),
            }],
            locals: Vec::new(),
            statements: Vec::new(),
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
}
