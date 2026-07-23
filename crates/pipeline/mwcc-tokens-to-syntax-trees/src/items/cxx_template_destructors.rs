//! Deferred weak destructors instantiated from inline class templates.
//!
//! Template member calls may disappear through inline expansion while MWCC
//! still emits a weak out-of-line fallback. This module owns recognition of
//! optional-storage lifetime templates and construction of those fallback
//! bodies; top-level orchestration only decides where requested weak bodies go.

use mwcc_core::Compilation;
use mwcc_syntax_trees::{Expression, Function, Parameter, Pointee, Statement, Type};

use crate::cxx::mangle_qualified_member_function;
use crate::parser::{Parser, TemplateFieldType, TemplateTypePattern};

use super::cxx_destructors;

struct OptionalDestructor {
    object_size: u32,
    storage_offset: u32,
    valid_offset: u32,
    leaf_destructor: String,
    wrapper_depth: usize,
}

/// Queue one weak out-of-line destructor for an instantiated optional-storage
/// type. Requests retain first field-declaration order; deferred emission later
/// reverses the generated-weak stream exactly once.
pub(crate) fn request_optional_destructor(
    parser: &mut Parser,
    concrete: &str,
) -> Compilation<()> {
    let Some(specification) = inspect_optional_destructor(parser, concrete)? else {
        return Ok(());
    };
    let scopes = concrete.split("::").collect::<Vec<_>>();
    let name = mangle_qualified_member_function(&scopes, "__dt", &[])?;
    if parser
        .cxx_deferred_weak_materialization_requests
        .iter()
        .any(|requested| requested == &name)
    {
        return Ok(());
    }

    let mut lifetime = lifetime_statements(&specification, 0);
    let [Statement::If {
        then_body,
        else_body,
        ..
    }] = lifetime.as_mut_slice()
    else {
        unreachable!("optional lifetime always has one outer object guard")
    };
    debug_assert!(else_body.is_empty());
    let body = std::mem::take(then_body);
    let object_size = specification.object_size;
    let mut function = Function {
        return_type: Type::Void,
        name: name.clone(),
        is_static: false,
        is_weak: true,
        text_deferred: false,
        peephole_disabled: false,
        parameters: vec![
            Parameter {
                parameter_type: Type::StructPointer {
                    element_size: object_size,
                },
                name: "this".to_string(),
            },
            Parameter {
                parameter_type: Type::Short,
                name: "__destroy".to_string(),
            },
        ],
        locals: Vec::new(),
        statements: body,
        guards: Vec::new(),
        return_expression: None,
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
    };
    let delete = cxx_destructors::delete_call(parser, concrete, object_size);
    cxx_destructors::wrap_written(
        &mut function,
        object_size,
        Vec::new(),
        Vec::new(),
        delete,
    );
    parser.cxx_inline_materializations.push(function);
    parser
        .cxx_deferred_weak_materialization_requests
        .push(name);
    Ok(())
}

/// Inline the same lifetime operation into a containing class destructor.
pub(crate) fn optional_member_lifetime(
    parser: &Parser,
    concrete: &str,
    object_offset: u32,
) -> Compilation<Option<Vec<Statement>>> {
    Ok(inspect_optional_destructor(parser, concrete)?
        .map(|specification| lifetime_statements(&specification, object_offset)))
}

fn inspect_optional_destructor(
    parser: &Parser,
    concrete: &str,
) -> Compilation<Option<OptionalDestructor>> {
    let Some((primary, encoded_argument)) = template_primary_and_argument(concrete) else {
        return Ok(None);
    };
    let Some(template) = parser.struct_templates.get(primary) else {
        return Ok(None);
    };
    if !parser
        .inline_template_members
        .contains(&(primary.to_string(), "__dt".to_string()))
        || !parser
            .inline_template_members
            .contains(&(primary.to_string(), "clear".to_string()))
    {
        return Ok(None);
    }
    let Some(storage) = template.fields.iter().find(|field| {
        matches!(field.field_type, TemplateFieldType::ParameterByteArray(0))
    }) else {
        return Ok(None);
    };
    let Some(valid) = template.fields.iter().find(|field| {
        matches!(field.field_type, TemplateFieldType::Concrete(Type::UnsignedChar))
            && field.alignment >= 4
    }) else {
        return Ok(None);
    };
    let Some(layout) = parser.structs.get(concrete) else {
        return Ok(None);
    };
    let Some(storage_field) = layout.fields.get(&storage.name) else {
        return Ok(None);
    };
    let Some(valid_field) = layout.fields.get(&valid.name) else {
        return Ok(None);
    };
    let argument = encoded_argument
        .trim_start_matches(|character: char| character.is_ascii_digit());
    let Some((leaf_class, wrapper_depth)) = template_destructor_leaf(parser, argument) else {
        return Ok(None);
    };
    let leaf_destructor = parser.mangle_typed_member_in_current_namespace(
        &leaf_class,
        "__dt",
        &[],
    )?;
    Ok(Some(OptionalDestructor {
        object_size: layout.size,
        storage_offset: storage_field.offset,
        valid_offset: valid_field.offset,
        leaf_destructor,
        wrapper_depth,
    }))
}

fn lifetime_statements(
    specification: &OptionalDestructor,
    object_offset: u32,
) -> Vec<Statement> {
    let object = || adjusted_this(object_offset + specification.storage_offset);
    let mut destruction = Statement::Expression(Expression::Call {
        name: specification.leaf_destructor.clone(),
        arguments: vec![object(), Expression::IntegerLiteral(0)],
    });
    for _ in 0..specification.wrapper_depth {
        destruction = Statement::If {
            condition: object(),
            then_body: vec![destruction],
            else_body: Vec::new(),
        };
    }
    let valid_offset = object_offset + specification.valid_offset;
    vec![Statement::If {
        condition: object(),
        then_body: vec![
            Statement::If {
                condition: Expression::Member {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: valid_offset,
                    member_type: Type::UnsignedChar,
                    index_stride: None,
                },
                then_body: vec![destruction],
                else_body: Vec::new(),
            },
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: valid_offset,
                    member_type: Type::UnsignedChar,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(0),
            },
        ],
        else_body: Vec::new(),
    }]
}

fn template_destructor_leaf(parser: &Parser, concrete: &str) -> Option<(String, usize)> {
    let mut primary = template_primary_and_argument(concrete)?.0;
    let mut wrapper_depth = 0;
    loop {
        let template = parser.struct_templates.get(primary)?;
        wrapper_depth += 1;
        match template.base.as_ref()? {
            TemplateTypePattern::Named(name) => {
                let destructible = parser.cxx_nonvirtual_destructor_classes.contains(name)
                    || parser.cxx_classes.get(name).is_some_and(|class| {
                        class.has_virtual_destructor || class.declares_destructor
                    });
                return destructible.then(|| (name.clone(), wrapper_depth));
            }
            TemplateTypePattern::Instance { name, .. } => primary = name,
            TemplateTypePattern::Parameter(_) => return None,
        }
    }
}

fn template_primary_and_argument(tag: &str) -> Option<(&str, &str)> {
    let open = tag.find('<')?;
    let argument = tag.get(open + 1..tag.len().checked_sub(1)?)?;
    let primary = tag[..open].rsplit("::").next()?;
    Some((primary, argument))
}

fn adjusted_this(offset: u32) -> Expression {
    if offset == 0 {
        Expression::Variable("this".to_string())
    } else {
        Expression::MemberAddress {
            base: Box::new(Expression::Variable("this".to_string())),
            offset,
            element: Pointee::UnsignedChar,
            index_stride: None,
        }
    }
}
