//! C++ class, ABI-data, and member-function DIEs for GC 4.x units.
//!
//! These records are a semantic dependency graph: modified callable types lead
//! into the class definition, which owns constructor/vtable/RTTI/destructor
//! siblings. Object-level fragment symbols are assigned later by `fragmented`.

use super::{attribute, data};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{
    Address, AttributeName, AttributeValue, Block, BlockRelocation, DebugEntry, DebugEntryId,
    DebugRecord, FundamentalType, LineRecord, Tag,
};
use mwcc_machine_code::MachineFunction;
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    CxxAbiClass, Function, FunctionSource, GlobalDeclaration, TranslationUnit, Type,
};

pub(super) fn matches(unit: &TranslationUnit, functions: &[&Function]) -> bool {
    if unit.cxx_abi_classes.len() != 1 || functions.len() != 2 {
        return false;
    }
    let class = &unit.cxx_abi_classes[0];
    functions[0].name == format!("__ct__{}Fv", class.encoded_name)
        && functions[1].name == format!("__dt__{}Fv", class.encoded_name)
        && class.bases.is_empty()
        && class.vtable_components.len() == 1
        && abi_global(unit, "__vt__", class).is_some()
        && abi_global(unit, "__RTTI__", class).is_some()
        && unit.aggregate_definitions.contains_key(&class.source_name)
}

pub(super) fn records(
    unit: &TranslationUnit,
    functions: &[&Function],
    layout: &FunctionLayout,
    first_id: DebugEntryId,
) -> Compilation<Vec<DebugRecord>> {
    if !matches(unit, functions) {
        return Err(Diagnostic::error(
            "debug-info: unsupported GC 4.1 class record plan",
        ));
    }
    let class = &unit.cxx_abi_classes[0];
    let definition = &unit.aggregate_definitions[&class.source_name];
    let vtable = abi_global(unit, "__vt__", class).expect("class match checked vtable");
    let rtti = abi_global(unit, "__RTTI__", class).expect("class match checked RTTI");
    let mut ids = IdAllocator(first_id.0);

    let modified_class = ids.entry();
    let modified_class_parameter = ids.entry();
    let modified_class_parameter_end = ids.entry();
    let modified_class_end = ids.entry();

    let modified_pointer_with_parameter = ids.entry();
    let pointer_parameter = ids.entry();
    let pointer_parameter_end = ids.entry();
    let modified_pointer_with_parameter_end = ids.entry();

    let modified_pointer = ids.entry();
    let modified_pointer_end = ids.entry();

    let final_modified_pointer = ids.entry();
    let final_class_parameter = ids.entry();
    let final_class_parameter_end = ids.entry();
    let final_modified_pointer_end = ids.entry();

    let class_id = ids.entry();
    let vptr_id = ids.entry();
    let member_ids = definition
        .members
        .iter()
        .map(|_| ids.entry())
        .collect::<Vec<_>>();
    let class_children_end = ids.entry();
    let constructor_id = ids.entry();
    let vtable_type_id = ids.entry();
    let vtable_id = ids.entry();
    let rtti_type_id = ids.entry();
    let rtti_id = ids.entry();
    let destructor_id = ids.entry();
    let this_id = ids.entry();
    let this_end = ids.entry();
    let function_end = ids.entry();

    let mut records = vec![
        modified_class_type(modified_class, modified_class_end, class_id),
        class_pointer_parameter(
            modified_class_parameter,
            modified_class_parameter_end,
            class_id,
            false,
        ),
        DebugRecord::Marker(modified_class_parameter_end),
        null(),
        DebugRecord::Marker(modified_class_end),
        modified_fundamental_pointer(
            modified_pointer_with_parameter,
            modified_pointer_with_parameter_end,
        ),
        fundamental_parameter(
            pointer_parameter,
            pointer_parameter_end,
            FundamentalType::SignedShort,
        ),
        DebugRecord::Marker(pointer_parameter_end),
        null(),
        DebugRecord::Marker(modified_pointer_with_parameter_end),
        modified_fundamental_pointer(modified_pointer, modified_pointer_end),
        DebugRecord::Marker(modified_pointer_end),
        modified_fundamental_pointer(final_modified_pointer, final_modified_pointer_end),
        class_pointer_parameter(
            final_class_parameter,
            final_class_parameter_end,
            class_id,
            false,
        ),
        DebugRecord::Marker(final_class_parameter_end),
        null(),
        DebugRecord::Marker(final_modified_pointer_end),
    ];

    records.push(DebugRecord::Entry(DebugEntry {
        id: class_id,
        tag: Tag::ClassType,
        attributes: vec![
            attribute(
                AttributeName::Sibling,
                AttributeValue::Reference(constructor_id),
            ),
            attribute(
                AttributeName::Name,
                AttributeValue::String(class.source_name.clone()),
            ),
            attribute(
                AttributeName::ByteSize,
                AttributeValue::Data4(definition.byte_size),
            ),
        ],
    }));
    let first_member = member_ids.first().copied().unwrap_or(class_children_end);
    records.push(member(
        vptr_id,
        first_member,
        "__vptr$",
        FundamentalType::Pointer,
        0,
    ));
    for (index, source_member) in definition.members.iter().enumerate() {
        let sibling = member_ids
            .get(index + 1)
            .copied()
            .unwrap_or(class_children_end);
        let type_attribute = data::member_type_attribute(
            source_member.declared_type,
            None,
            source_member.source_fundamental,
        )?;
        records.push(DebugRecord::Entry(DebugEntry {
            id: member_ids[index],
            tag: Tag::Member,
            attributes: vec![
                attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
                attribute(
                    AttributeName::Name,
                    AttributeValue::String(source_member.name.clone()),
                ),
                type_attribute,
                attribute(
                    AttributeName::MwMemberFlags,
                    AttributeValue::String(String::new()),
                ),
                member_location(source_member.offset),
            ],
        }));
    }
    records.extend([DebugRecord::Marker(class_children_end), null()]);

    records.push(member_function(
        functions[0],
        constructor_id,
        vtable_type_id,
        class_id,
        Some(vtable_id),
        layout.offsets[0],
        layout.sizes[0],
    ));
    records.push(anonymous_struct(
        vtable_type_id,
        vtable_id,
        byte_size(vtable)?,
    ));
    records.push(abi_global_entry(
        vtable,
        vtable_id,
        rtti_type_id,
        vtable_type_id,
    ));
    records.push(anonymous_struct(rtti_type_id, rtti_id, byte_size(rtti)?));
    records.push(abi_global_entry(rtti, rtti_id, destructor_id, rtti_type_id));
    records.push(member_function(
        functions[1],
        destructor_id,
        function_end,
        class_id,
        None,
        layout.offsets[1],
        layout.sizes[1],
    ));
    records.push(class_pointer_parameter(this_id, this_end, class_id, true));
    records.extend([
        DebugRecord::Marker(this_end),
        null(),
        DebugRecord::Marker(function_end),
        null(),
        null(),
    ]);
    Ok(records)
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    if functions.len() != 2
        || machine_functions.len() != 2
        || functions[0].1.statement_lines.len() != 1
        || !functions[1].1.statement_lines.is_empty()
        || layout.sizes[0] < 12
    {
        return Err(Diagnostic::error(
            "debug-info: unsupported GC 4.1 class line plan",
        ));
    }
    Ok(vec![
        line(functions[0].1.body_start_line, layout.offsets[0]),
        line(
            functions[0].1.statement_lines[0],
            layout.offsets[0] + layout.sizes[0] - 12,
        ),
        line(
            functions[0].1.body_end_line,
            layout.offsets[0] + layout.sizes[0] - 4,
        ),
        line(functions[1].1.body_start_line, layout.offsets[1]),
    ])
}

fn line(source_line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line: source_line,
        column: u16::MAX,
        address_delta,
    }
}

fn modified_class_type(
    id: DebugEntryId,
    sibling: DebugEntryId,
    class: DebugEntryId,
) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::ModifiedType,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            modified_class_attribute(class, &[2]),
        ],
    })
}

fn modified_fundamental_pointer(id: DebugEntryId, sibling: DebugEntryId) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::ModifiedType,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            pointer_type(),
        ],
    })
}

fn class_pointer_parameter(
    id: DebugEntryId,
    sibling: DebugEntryId,
    class: DebugEntryId,
    named_this: bool,
) -> DebugRecord {
    let mut attributes = vec![attribute(
        AttributeName::Sibling,
        AttributeValue::Reference(sibling),
    )];
    if named_this {
        attributes.push(attribute(
            AttributeName::Name,
            AttributeValue::String("this".into()),
        ));
        attributes.push(modified_class_attribute(class, &[3, 1]));
        attributes.push(attribute(
            AttributeName::Location,
            AttributeValue::Block2(vec![1, 0, 0, 0, 31]),
        ));
    } else {
        attributes.push(modified_class_attribute(class, &[2]));
    }
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::FormalParameter,
        attributes,
    })
}

fn fundamental_parameter(
    id: DebugEntryId,
    sibling: DebugEntryId,
    parameter_type: FundamentalType,
) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::FormalParameter,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(
                AttributeName::FundamentalType,
                AttributeValue::Data2(parameter_type as u16),
            ),
        ],
    })
}

fn modified_class_attribute(class: DebugEntryId, modifiers: &[u8]) -> mwcc_dwarf1::Attribute {
    let mut bytes = modifiers.to_vec();
    let relocation_offset = bytes.len() as u32;
    bytes.extend_from_slice(&[0; 4]);
    attribute(
        AttributeName::ModifiedUserDefinedType,
        AttributeValue::RelocatableBlock2(Block {
            bytes,
            relocations: vec![BlockRelocation {
                offset: relocation_offset,
                address: Address::debug_entry(class),
            }],
        }),
    )
}

fn pointer_type() -> mwcc_dwarf1::Attribute {
    attribute(
        AttributeName::FundamentalType,
        AttributeValue::Data2(FundamentalType::Pointer as u16),
    )
}

fn member(
    id: DebugEntryId,
    sibling: DebugEntryId,
    name: &str,
    member_type: FundamentalType,
    offset: u32,
) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::Member,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(AttributeName::Name, AttributeValue::String(name.into())),
            attribute(
                AttributeName::FundamentalType,
                AttributeValue::Data2(member_type as u16),
            ),
            attribute(
                AttributeName::MwMemberFlags,
                AttributeValue::String(String::new()),
            ),
            member_location(offset),
        ],
    })
}

fn member_location(offset: u32) -> mwcc_dwarf1::Attribute {
    let mut bytes = vec![4];
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.push(7);
    attribute(AttributeName::Location, AttributeValue::Block2(bytes))
}

fn member_function(
    function: &Function,
    id: DebugEntryId,
    sibling: DebugEntryId,
    class: DebugEntryId,
    vtable: Option<DebugEntryId>,
    text_offset: u32,
    text_size: u32,
) -> DebugRecord {
    let short_name = if function.name.starts_with("__ct__") {
        "__ct"
    } else {
        "__dt"
    };
    let mut attributes = vec![
        attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
        attribute(
            AttributeName::Name,
            AttributeValue::String(short_name.into()),
        ),
        attribute(
            AttributeName::MwLinkageName,
            AttributeValue::String(function.name.clone()),
        ),
        attribute(AttributeName::Member, AttributeValue::Reference(class)),
        pointer_type(),
        attribute(
            AttributeName::LowPc,
            AttributeValue::Address(Address::external_with_addend(".text", text_offset as i32)),
        ),
        attribute(
            AttributeName::HighPc,
            AttributeValue::Address(Address::external_with_addend(
                ".text",
                (text_offset + text_size) as i32,
            )),
        ),
    ];
    if let Some(vtable) = vtable {
        attributes.push(attribute(
            AttributeName::MwVtableElement,
            AttributeValue::Reference(vtable),
        ));
    }
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::GlobalSubroutine,
        attributes,
    })
}

fn anonymous_struct(id: DebugEntryId, sibling: DebugEntryId, byte_size: u32) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::StructureType,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(AttributeName::ByteSize, AttributeValue::Data4(byte_size)),
        ],
    })
}

fn abi_global_entry(
    global: &GlobalDeclaration,
    id: DebugEntryId,
    sibling: DebugEntryId,
    type_id: DebugEntryId,
) -> DebugRecord {
    DebugRecord::Entry(DebugEntry {
        id,
        tag: Tag::GlobalVariable,
        attributes: vec![
            attribute(AttributeName::Sibling, AttributeValue::Reference(sibling)),
            attribute(
                AttributeName::Name,
                AttributeValue::String(global.name.clone()),
            ),
            attribute(
                AttributeName::UserDefinedType,
                AttributeValue::Reference(type_id),
            ),
            attribute(
                AttributeName::Location,
                AttributeValue::RelocatableBlock2(Block {
                    bytes: vec![3, 0, 0, 0, 0],
                    relocations: vec![BlockRelocation {
                        offset: 1,
                        address: Address::external(&global.name),
                    }],
                }),
            ),
        ],
    })
}

fn abi_global<'a>(
    unit: &'a TranslationUnit,
    prefix: &str,
    class: &CxxAbiClass,
) -> Option<&'a GlobalDeclaration> {
    let name = format!("{prefix}{}", class.encoded_name);
    unit.globals.iter().find(|global| global.name == name)
}

fn byte_size(global: &GlobalDeclaration) -> Compilation<u32> {
    match global.declared_type {
        Type::Struct { size, .. } => Ok(size),
        _ => Err(Diagnostic::error(format!(
            "debug-info: C++ ABI global '{}' is not aggregate data",
            global.name
        ))),
    }
}

fn null() -> DebugRecord {
    DebugRecord::Raw(vec![0, 0, 0, 4])
}

struct IdAllocator(u32);

impl IdAllocator {
    fn entry(&mut self) -> DebugEntryId {
        let id = DebugEntryId(self.0);
        self.0 += 1;
        id
    }
}
