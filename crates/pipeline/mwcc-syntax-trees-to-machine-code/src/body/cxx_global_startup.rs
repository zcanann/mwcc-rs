//! C++ namespace-scope object startup.
//!
//! Once an inline default constructor has expanded, a startup routine contains
//! its ordered vptr stores followed by `__register_global_object`.  MWCC treats
//! that sequence as one scheduling region: the two vtable addresses fill the
//! linkage latency slots, while the destructor and destructor-record addresses
//! are interleaved with the stores.  Keeping the semantic recognizer here avoids
//! teaching unrelated address/store/call emitters about this ABI transaction.

#[allow(unused_imports)]
use super::*;

struct CxxGlobalStartup<'a> {
    object: &'a str,
    base_vtable: &'a str,
    derived_vtable: &'a str,
    destructor: &'a str,
    destructor_record: &'a str,
}

fn addressed_variable(expression: &Expression) -> Option<&str> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Variable(name) = operand.as_ref() else {
        return None;
    };
    Some(name)
}

fn vptr_store(statement: &Statement) -> Option<(&str, &str)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let Expression::Member {
        base,
        offset: 0,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = target
    else {
        return None;
    };
    let object = addressed_variable(base)?;
    let vtable = addressed_variable(value)?;
    vtable.starts_with("__vt__").then_some((object, vtable))
}

fn classify(function: &Function) -> Option<CxxGlobalStartup<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [base_store, derived_store, Statement::Expression(Expression::Call {
        name,
        arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if name != "__register_global_object" {
        return None;
    }
    let (object, base_vtable) = vptr_store(base_store)?;
    let (derived_object, derived_vtable) = vptr_store(derived_store)?;
    let [registered_object, destructor, destructor_record] = arguments.as_slice() else {
        return None;
    };
    let registered_object = addressed_variable(registered_object)?;
    let destructor = addressed_variable(destructor)?;
    let destructor_record = addressed_variable(destructor_record)?;
    if object != derived_object
        || object != registered_object
        || base_vtable == derived_vtable
    {
        return None;
    }
    Some(CxxGlobalStartup {
        object,
        base_vtable,
        derived_vtable,
        destructor,
        destructor_record,
    })
}

impl Generator {
    pub(crate) fn try_cxx_global_startup(&mut self, function: &Function) -> Compilation<bool> {
        let Some(startup) = classify(function) else {
            return Ok(false);
        };

        // Startup analysis creates the destructor record before it resolves
        // the object's zero-storage symbol. These are both local symbols, so
        // retain that creation order independently of text-reference order.
        self.output.local_symbol_order = vec![
            startup.destructor_record.to_string(),
            startup.object.to_string(),
        ];
        // The constructor transaction resolves vtable data before the
        // registration call's function designators. Keep that discovery order
        // explicit: the source AST only sees the constructor call until after
        // inline expansion and is not an adequate symbol-order proxy.
        self.output.symbol_order = vec![
            startup.object.to_string(),
            startup.base_vtable.to_string(),
            startup.derived_vtable.to_string(),
            startup.destructor_record.to_string(),
            startup.destructor.to_string(),
            "__register_global_object".to_string(),
        ];
        self.output.defined_data_precedes_defined_functions = true;
        self.emit_plain_nonleaf_prologue();

        self.emit_address_high(4, startup.base_vtable);
        self.emit_address_high(3, startup.derived_vtable);

        self.record_relocation(RelocationKind::Addr16Lo, startup.base_vtable);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 0,
        });
        self.emit_address_high(4, startup.destructor);

        self.record_relocation(RelocationKind::EmbSda21, startup.object);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });

        self.record_relocation(RelocationKind::Addr16Lo, startup.derived_vtable);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.emit_address_high(3, startup.destructor_record);

        self.record_relocation(RelocationKind::Addr16Lo, startup.destructor);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });

        self.record_relocation(RelocationKind::EmbSda21, startup.object);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });

        self.record_relocation(RelocationKind::Addr16Lo, startup.destructor_record);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });

        self.record_relocation(RelocationKind::EmbSda21, startup.object);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        });

        self.record_relocation(RelocationKind::Rel24, "__register_global_object");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "__register_global_object".to_string(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(name: &str) -> Expression {
        Expression::AddressOf {
            operand: Box::new(Expression::Variable(name.to_string())),
        }
    }

    fn store(object: &str, vtable: &str) -> Statement {
        Statement::Store {
            target: Expression::Member {
                base: Box::new(address(object)),
                offset: 0,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: address(vtable),
        }
    }

    fn startup(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "__sinit_probe_cpp".to_string(),
            is_static: true,
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

    #[test]
    fn recognizes_expanded_vptr_stores_and_registration() {
        let function = startup(vec![
            store("object", "__vt__4Base"),
            store("object", "__vt__7Derived"),
            Statement::Expression(Expression::Call {
                name: "__register_global_object".to_string(),
                arguments: vec![address("object"), address("__dt__7DerivedFv"), address("@3")],
            }),
        ]);

        let startup = classify(&function).expect("startup shape");
        assert_eq!(startup.object, "object");
        assert_eq!(startup.base_vtable, "__vt__4Base");
        assert_eq!(startup.derived_vtable, "__vt__7Derived");
        assert_eq!(startup.destructor, "__dt__7DerivedFv");
        assert_eq!(startup.destructor_record, "@3");
    }

    #[test]
    fn rejects_registration_for_a_different_object() {
        let function = startup(vec![
            store("object", "__vt__4Base"),
            store("object", "__vt__7Derived"),
            Statement::Expression(Expression::Call {
                name: "__register_global_object".to_string(),
                arguments: vec![address("other"), address("__dt__7DerivedFv"), address("@3")],
            }),
        ]);

        assert!(classify(&function).is_none());
    }
}
