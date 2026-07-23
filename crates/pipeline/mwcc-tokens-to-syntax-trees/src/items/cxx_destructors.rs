//! Materialization of compiler-generated C++ deleting destructors.
//!
//! Vtable ownership decides which weak inline bodies become linkable code.
//! Keep that dependency closure and the implicit-destructor body shape here,
//! separate from ordinary source function parsing.

use mwcc_core::Compilation;
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, GlobalDeclaration, Parameter, Pointee, Statement, Type,
};
use std::collections::HashSet;

use crate::cxx::{
    encode_qualified_scope, mangle_qualified_member_function, BaseClass, ClassLayout,
};
use crate::parser::Parser;

pub(super) struct DeleteCall {
    name: String,
    object_size: Option<u32>,
}

/// Rebuild trivial written destructors after the complete class graph exists,
/// and add implicit derived destructors required by an emitted vtable.
pub(super) fn prepare_required(
    parser: &mut Parser,
    globals: &[GlobalDeclaration],
    functions: &[Function],
) -> Compilation<()> {
    let required = globals
        .iter()
        .flat_map(|global| global.data_relocations.iter())
        .map(|(_, target, _)| target.as_str())
        .filter(|target| target.contains("__dt__"))
        .collect::<HashSet<_>>();
    let emitted = functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<HashSet<_>>();
    let classes = parser.cxx_class_declaration_order.clone();

    for scope in classes {
        let scopes = scope.split("::").collect::<Vec<_>>();
        let destructor = mangle_qualified_member_function(&scopes, "__dt", &[])?;
        if !required.contains(destructor.as_str()) || emitted.contains(destructor.as_str()) {
            continue;
        }
        if let Some(index) = parser
            .cxx_inline_materializations
            .iter()
            .position(|function| function.name == destructor)
        {
            if is_trivial_generated_destructor(&parser.cxx_inline_materializations[index])
                && bases_are_trivial(parser, &scope)?
            {
                let mut rebuilt = build(parser, &scope, destructor)?;
                let original = &parser.cxx_inline_materializations[index];
                rebuilt.section = original.section.clone();
                rebuilt.preceded_by_asm = original.preceded_by_asm;
                parser.cxx_inline_materializations[index] = rebuilt;
            }
            continue;
        }
        let class = &parser.cxx_classes[&scope];
        if !class.declares_destructor && bases_are_trivial(parser, &scope)? {
            parser
                .cxx_inline_materializations
                .push(build(parser, &scope, destructor)?);
        }
    }
    Ok(())
}

/// Ensure one inline or implicit destructor requested by a virtual delete is
/// available for immediate weak emission after its caller.
pub(super) fn prepare_requested(
    parser: &mut Parser,
    scope: &str,
) -> Compilation<Option<String>> {
    let scopes = scope.split("::").collect::<Vec<_>>();
    let destructor = mangle_qualified_member_function(&scopes, "__dt", &[])?;
    if let Some(index) = parser
        .cxx_inline_materializations
        .iter()
        .position(|function| function.name == destructor)
    {
        if is_trivial_generated_destructor(&parser.cxx_inline_materializations[index])
            && bases_are_trivial(parser, scope)?
        {
            let original = &parser.cxx_inline_materializations[index];
            let section = original.section.clone();
            let preceded_by_asm = original.preceded_by_asm;
            let mut rebuilt = build(parser, scope, destructor.clone())?;
            rebuilt.section = section;
            rebuilt.preceded_by_asm = preceded_by_asm;
            parser.cxx_inline_materializations[index] = rebuilt;
        }
        return Ok(Some(destructor));
    }
    let Some(class) = parser.cxx_classes.get(scope) else {
        return Ok(None);
    };
    if class.declares_destructor || !bases_are_trivial(parser, scope)? {
        return Ok(None);
    }
    parser
        .cxx_inline_materializations
        .push(build(parser, scope, destructor.clone())?);
    Ok(Some(destructor))
}

fn bases_are_trivial(parser: &Parser, scope: &str) -> Compilation<bool> {
    let Some(class) = parser.cxx_classes.get(scope) else {
        return Ok(false);
    };
    for base in class.bases.iter().filter(|base| !base.is_virtual) {
        let Some(base_class) = parser.cxx_classes.get(&base.name) else {
            return Ok(false);
        };
        if !base_class.has_virtual_destructor {
            continue;
        }
        let scopes = base.name.split("::").collect::<Vec<_>>();
        let destructor = mangle_qualified_member_function(&scopes, "__dt", &[])?;
        let Some(candidate) = parser
            .cxx_inline_materializations
            .iter()
            .find(|function| function.name == destructor)
        else {
            return Ok(false);
        };
        if !is_trivial_generated_destructor(candidate) || !bases_are_trivial(parser, &base.name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn build(parser: &Parser, scope: &str, name: String) -> Compilation<Function> {
    let class = &parser.cxx_classes[scope];
    let size = parser.structs.get(scope).map_or(0, |layout| layout.size);
    let mut body = vptr_stores(scope, class, 0)?;
    for base in class.bases.iter().rev().filter(|base| !base.is_virtual) {
        if let Some(call) = base_destructor_call(parser, base)? {
            body.push(call);
        }
    }
    body.push(deleting_guard(delete_call(parser, scope, size)));

    Ok(Function {
        return_type: Type::StructPointer { element_size: size },
        name,
        is_static: false,
        is_weak: true,
        text_deferred: false,
        peephole_disabled: false,
        parameters: vec![
            Parameter {
                parameter_type: Type::StructPointer { element_size: size },
                name: "this".to_string(),
            },
            Parameter {
                parameter_type: Type::Short,
                name: "__destroy".to_string(),
            },
        ],
        locals: Vec::new(),
        statements: vec![Statement::If {
            condition: Expression::Variable("this".to_string()),
            then_body: body,
            else_body: Vec::new(),
        }],
        guards: Vec::new(),
        return_expression: Some(Expression::Variable("this".to_string())),
        section: None,
        preceded_by_asm: false,
        asm_body: None,
        inline_asm_blocks: Vec::new(),
        force_active: false,
    })
}

fn base_destructor_call(parser: &Parser, base: &BaseClass) -> Compilation<Option<Statement>> {
    let Some(class) = parser.cxx_classes.get(&base.name) else {
        return Ok(None);
    };
    if !class.has_virtual_destructor {
        return Ok(None);
    }
    let scopes = base.name.split("::").collect::<Vec<_>>();
    let name = mangle_qualified_member_function(&scopes, "__dt", &[])?;
    let object = if base.offset == 0 {
        Expression::Variable("this".to_string())
    } else {
        Expression::MemberAddress {
            base: Box::new(Expression::Variable("this".to_string())),
            offset: base.offset,
            element: Pointee::UnsignedChar,
            index_stride: None,
        }
    };
    Ok(Some(Statement::Expression(Expression::Call {
        name,
        arguments: vec![object, Expression::IntegerLiteral(0)],
    })))
}

fn vptr_stores(scope: &str, class: &ClassLayout, object_bias: u32) -> Compilation<Vec<Statement>> {
    let scopes = scope.split("::").collect::<Vec<_>>();
    let vtable = format!("__vt__{}", encode_qualified_scope(&scopes)?);
    let mut table_offset = 0u32;
    Ok(class
        .vtable_components
        .iter()
        .map(|component| {
            let address = Expression::AddressOf {
                operand: Box::new(Expression::Variable(vtable.clone())),
            };
            let value = if table_offset == 0 {
                address
            } else {
                Expression::MemberAddress {
                    base: Box::new(address),
                    offset: table_offset,
                    element: Pointee::UnsignedChar,
                    index_stride: None,
                }
            };
            table_offset += 8 + component.virtual_slots.max(1) as u32 * 4;
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".to_string())),
                    offset: object_bias + component.vptr_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value,
            }
        })
        .collect())
}

pub(super) fn delete_call(parser: &Parser, scope: &str, object_size: u32) -> DeleteCall {
    let mut owner = Some(scope);
    while let Some(class_name) = owner {
        if let Some((name, arity)) = parser.cxx_class_deletes.get(class_name) {
            return DeleteCall {
                name: name.clone(),
                object_size: (*arity >= 2).then_some(object_size),
            };
        }
        owner = parser
            .cxx_classes
            .get(class_name)
            .and_then(|class| class.bases.iter().find(|base| !base.is_virtual))
            .map(|base| base.name.as_str());
    }
    DeleteCall {
        name: parser
            .cxx_delete_forwarder
            .clone()
            .unwrap_or_else(|| "__dl__FPv".to_string()),
        object_size: None,
    }
}

fn deleting_guard(delete: DeleteCall) -> Statement {
    let mut arguments = vec![Expression::Variable("this".to_string())];
    if let Some(size) = delete.object_size {
        arguments.push(Expression::IntegerLiteral(i64::from(size)));
    }
    Statement::If {
        condition: Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(Expression::Variable("__destroy".to_string())),
            right: Box::new(Expression::IntegerLiteral(0)),
        },
        then_body: vec![Statement::Expression(Expression::Call {
            name: delete.name,
            arguments,
        })],
        else_body: Vec::new(),
    }
}

/// Wrap a source-written destructor body in MWCC's complete-object deleting
/// ABI. The source signature remains `~T()`, while executable IR receives the
/// hidden signed-short flag and returns `this` after optional lifetime work.
pub(super) fn wrap_written(
    function: &mut Function,
    object_size: u32,
    mut before_source: Vec<Statement>,
    mut after_source: Vec<Statement>,
    delete: DeleteCall,
) {
    let mut body = Vec::new();
    body.append(&mut before_source);
    body.append(&mut function.statements);
    body.append(&mut after_source);
    body.push(deleting_guard(delete));

    function.return_type = Type::StructPointer {
        element_size: object_size,
    };
    function.statements = vec![Statement::If {
        condition: Expression::Variable("this".to_string()),
        then_body: body,
        else_body: Vec::new(),
    }];
    function.return_expression = Some(Expression::Variable("this".to_string()));
}

/// The ordinary parser has already inserted ABI vptr/base/delete statements.
/// A body containing only those statements corresponds to a written `{}` and
/// can be safely rebuilt once all class layouts are available.
fn is_trivial_generated_destructor(function: &Function) -> bool {
    let [Statement::If {
        then_body,
        else_body,
        ..
    }] = function.statements.as_slice()
    else {
        return false;
    };
    let Some((delete, lifetime)) = then_body.split_last() else {
        return false;
    };
    else_body.is_empty()
        && is_generated_delete_guard(delete)
        && lifetime.iter().all(|statement| match statement {
            Statement::Store { value, .. } => expression_mentions_vtable(value),
            Statement::Expression(Expression::Call { name, .. }) => name.contains("__dt__"),
            _ => false,
        })
}

fn is_generated_delete_guard(statement: &Statement) -> bool {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        && matches!(right.as_ref(), Expression::IntegerLiteral(0))
        && matches!(then_body.as_slice(), [Statement::Expression(Expression::Call { arguments, .. })]
            if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == "this")
                || matches!(arguments.as_slice(), [Expression::Variable(name), Expression::IntegerLiteral(_)] if name == "this"))
        && else_body.is_empty()
}

fn expression_mentions_vtable(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(name) => name.starts_with("__vt__"),
        Expression::AddressOf { operand } => expression_mentions_vtable(operand),
        Expression::MemberAddress { base, .. } => expression_mentions_vtable(base),
        _ => false,
    }
}
