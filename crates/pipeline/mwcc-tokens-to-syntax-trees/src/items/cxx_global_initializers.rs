//! Namespace-scope C++ object initialization.
//!
//! Declaration parsing records only storage and class identity. Once the full
//! translation unit has been seen, this module closes the constructor,
//! destructor, and vtable dependencies and returns the startup statements that
//! belong in the unit's synthesized `__sinit` function.

use super::{cxx_destructors, cxx_vtables};
use crate::cxx::encode_qualified_scope;
use crate::parser::{Parser, PendingGlobalInitializer};
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Expression, GlobalDeclaration, Statement, Type};

pub(super) struct Materialization {
    pub(super) statements: Vec<Statement>,
    pub(super) destructor_records: Vec<String>,
}

pub(super) fn materialize(
    parser: &mut Parser,
    globals: &mut Vec<GlobalDeclaration>,
) -> Compilation<Materialization> {
    let initializers = std::mem::take(&mut parser.pending_global_initializers);
    let mut statements = Vec::new();
    let mut destructor_records = Vec::new();

    for initializer in initializers {
        let PendingGlobalInitializer::CxxObject {
            storage_name,
            class_name,
        } = initializer
        else {
            let PendingGlobalInitializer::ArrayElement {
                array,
                index,
                expression,
            } = initializer
            else {
                unreachable!()
            };
            statements.push(Statement::Store {
                target: Expression::Index {
                    base: Box::new(Expression::Variable(array)),
                    index: Box::new(Expression::IntegerLiteral(index as i64)),
                },
                value: expression,
            });
            continue;
        };
        let class_name = parser
            .resolve_scoped_cxx_class_name(&class_name)
            .unwrap_or(class_name);
        let constructor = if parser.has_declared_default_constructor(&class_name) {
            Some(parser.resolve_placement_constructor(&class_name, &[])?)
        } else {
            parser.ensure_implicit_default_constructor(&class_name)?
        };
        let destructor = cxx_destructors::prepare_requested(parser, &class_name)?;

        if constructor.is_none() && destructor.is_none() {
            continue;
        }

        if let Some(constructor) = constructor {
            statements.push(Statement::Expression(Expression::Call {
                name: constructor,
                arguments: vec![address_of(&storage_name)],
            }));
        }

        let Some(destructor) = destructor else {
            continue;
        };
        let class = parser.cxx_classes.get(&class_name).cloned();
        if let Some(class) = class.filter(|class| !class.vtable_components.is_empty()) {
            let scopes = class_name.split("::").collect::<Vec<_>>();
            let vtable = format!("__vt__{}", encode_qualified_scope(&scopes)?);
            if !globals.iter().any(|global| global.name == vtable) {
                let mut table = cxx_vtables::global(&class, vtable, Some(&destructor));
                table.is_weak = true;
                // Namespace-scope construction creates the deleting-destructor
                // dependency before it finishes walking ordinary virtual
                // slots. The object writer emits each table's relocation stream
                // in reverse creation order, so retain that analysis order here.
                table.data_relocations.reverse();
                globals.push(table);
            }
        }

        let record = format!("@@global_destructor_record{}", destructor_records.len());
        globals.push(GlobalDeclaration {
            declared_type: Type::Struct { size: 12, align: 4 },
            source_fundamental: None,
            name: record.clone(),
            is_extern: false,
            is_static: true,
            is_volatile: false,
            is_weak: false,
            non_static_functions_before: 0,
            functions_before: 0,
            array_length: None,
            array_length_inferred: false,
            initializer: None,
            is_const: false,
            address_initializer: None,
            data_bytes: None,
            data_relocations: Vec::new(),
            section: None,
            attribute_alignment: None,
        });
        statements.push(Statement::Expression(Expression::Call {
            name: "__register_global_object".to_string(),
            arguments: vec![
                address_of(&storage_name),
                address_of(&destructor),
                address_of(&record),
            ],
        }));
        destructor_records.push(record);
    }

    Ok(Materialization {
        statements,
        destructor_records,
    })
}

fn address_of(name: &str) -> Expression {
    Expression::AddressOf {
        operand: Box::new(Expression::Variable(name.to_string())),
    }
}
