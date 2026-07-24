//! Linkage-first lowering for a fully inlined polymorphic constructor chain.
//!
//! Retained C++ inline bodies expose every construction phase in the AST:
//! base vptr installs, zeroed links, a shared name literal, the remaining
//! out-of-line initializer, and the most-derived payload. Build 163 schedules
//! that transaction as one unit. Keeping the recognizer here avoids teaching
//! ordinary store lowering about constructor lifetime phases.

use std::collections::HashMap;

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{Expression, Function, Statement, Type};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention, PlainLinkageEpilogueStyle};

struct Shape {
    first_vtable: String,
    second_vtable: String,
    zero_offsets: [i16; 3],
    name_offset: i16,
    name: Vec<u8>,
    third_vtable: String,
    initializer: String,
    derived_vtable: String,
    payload_offset: i16,
}

pub(crate) fn lower(
    function: &Function,
    source_inline_string_symbols: &HashMap<Vec<u8>, String>,
    config: CompilerConfig,
) -> Option<MachineFunction> {
    let behavior = Behavior::resolve(&config);
    if behavior.frame_convention != FrameConvention::LinkageFirst
        || behavior.plain_linkage_epilogue_style
            != PlainLinkageEpilogueStyle::ReloadBeforeStackRestore
        || config.flags.ipa_file
    {
        return None;
    }
    let shape = recognize(function)?;
    let string_target = source_inline_string_symbols
        .get(&shape.name)
        .cloned()
        .unwrap_or_else(|| "@@str0".to_string());

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::load_immediate_shifted(5, 0),
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        },
        Instruction::load_immediate(6, 0),
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -24,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 16,
        },
        Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::StoreWord {
            s: 6,
            a: 30,
            offset: shape.zero_offsets[2],
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 6,
            a: 30,
            offset: shape.zero_offsets[1],
        },
        Instruction::Or { a: 4, s: 5, b: 5 },
        Instruction::StoreWord {
            s: 6,
            a: 30,
            offset: shape.zero_offsets[0],
        },
        Instruction::StoreWord {
            s: 5,
            a: 30,
            offset: shape.name_offset,
        },
        Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        },
        Instruction::BranchAndLink {
            target: shape.initializer.clone(),
        },
        Instruction::load_immediate_shifted(3, 0),
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        },
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::StoreWord {
            s: 31,
            a: 30,
            offset: shape.payload_offset,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 20,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 16,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        relocation(1, RelocationKind::Addr16Ha, &shape.first_vtable),
        relocation(3, RelocationKind::Addr16Lo, &shape.first_vtable),
        relocation(10, RelocationKind::Addr16Ha, &shape.second_vtable),
        relocation(12, RelocationKind::Addr16Lo, &shape.second_vtable),
        relocation(13, RelocationKind::Addr16Ha, &string_target),
        relocation(15, RelocationKind::Addr16Lo, &string_target),
        relocation(16, RelocationKind::Addr16Ha, &shape.third_vtable),
        relocation(18, RelocationKind::Addr16Lo, &shape.third_vtable),
        relocation(25, RelocationKind::Rel24, &shape.initializer),
        relocation(26, RelocationKind::Addr16Ha, &shape.derived_vtable),
        relocation(27, RelocationKind::Addr16Lo, &shape.derived_vtable),
    ];
    output.symbol_order = vec![
        shape.first_vtable,
        shape.second_vtable,
        string_target,
        shape.third_vtable,
        shape.initializer.clone(),
        shape.derived_vtable,
    ];
    output.referenced_function_symbols = vec![shape.initializer.clone()];
    output.implicit_external_callees = vec![shape.initializer];
    output.string_literals.push(shape.name.clone());
    if let Some(symbol) = source_inline_string_symbols.get(&shape.name) {
        output.string_literal_symbols.insert(0, symbol.clone());
    }
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if config.flags.cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 2,
            saved_fpr_count: 0,
            uses_fpu: false,
        });
    }
    Some(output)
}

fn recognize(function: &Function) -> Option<Shape> {
    if !function.name.starts_with("__ct__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || function.locals.len() != 0
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let [first, second, zero_chain, name_store, third, initializer, derived, payload] =
        function.statements.as_slice()
    else {
        return None;
    };
    let first_vtable = vptr(first)?;
    let second_vtable = vptr(second)?;
    let zero_offsets = chained_zero_offsets(zero_chain)?;
    let (name_offset, name) = string_member_store(name_store)?;
    let third_vtable = vptr(third)?;
    let Statement::Expression(Expression::Call {
        name: initializer,
        arguments,
    }) = initializer
    else {
        return None;
    };
    if !matches!(
        arguments.as_slice(),
        [Expression::Variable(this), Expression::StringLiteral(argument)]
            if this == "this" && argument == &name
    ) {
        return None;
    }
    let derived_vtable = vptr(derived)?;
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: payload_offset,
                index_stride: None,
                ..
            },
        value: Expression::Variable(payload_value),
    } = payload
    else {
        return None;
    };
    if !matches!(base.as_ref(), Expression::Variable(this) if this == "this")
        || payload_value != &function.parameters[1].name
    {
        return None;
    }
    Some(Shape {
        first_vtable,
        second_vtable,
        zero_offsets,
        name_offset,
        name,
        third_vtable,
        initializer: initializer.clone(),
        derived_vtable,
        payload_offset: i16::try_from(*payload_offset).ok()?,
    })
}

fn vptr(statement: &Statement) -> Option<String> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: 0,
                index_stride: None,
                ..
            },
        value: Expression::AddressOf { operand },
    } = statement
    else {
        return None;
    };
    let Expression::Variable(this) = base.as_ref() else {
        return None;
    };
    let Expression::Variable(vtable) = operand.as_ref() else {
        return None;
    };
    (this == "this" && vtable.starts_with("__vt__")).then(|| vtable.clone())
}

fn chained_zero_offsets(statement: &Statement) -> Option<[i16; 3]> {
    let Statement::Store {
        target: outer,
        value,
    } = statement
    else {
        return None;
    };
    let Expression::Assign {
        target: middle,
        value,
    } = value
    else {
        return None;
    };
    let Expression::Assign {
        target: inner,
        value,
    } = value.as_ref()
    else {
        return None;
    };
    if !matches!(value.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    Some([
        this_member_offset(outer)?,
        this_member_offset(middle)?,
        this_member_offset(inner)?,
    ])
}

fn string_member_store(statement: &Statement) -> Option<(i16, Vec<u8>)> {
    let Statement::Store {
        target,
        value: Expression::StringLiteral(bytes),
    } = statement
    else {
        return None;
    };
    Some((this_member_offset(target)?, bytes.clone()))
}

fn this_member_offset(expression: &Expression) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(this) if this == "this")
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn relocation(index: usize, kind: RelocationKind, target: &str) -> Relocation {
    Relocation {
        instruction_index: index,
        kind,
        target: RelocationTarget::External(target.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Parameter, Pointee};
    use mwcc_versions::GC_1_2_5N;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("this".into())),
            offset,
            member_type: Type::Int,
            index_stride: None,
        }
    }

    fn vptr_store(symbol: &str) -> Statement {
        Statement::Store {
            target: member(0),
            value: Expression::AddressOf {
                operand: Box::new(Expression::Variable(symbol.into())),
            },
        }
    }

    fn constructor() -> Function {
        let name = b"boss".to_vec();
        Function {
            return_type: Type::StructPointer { element_size: 24 },
            name: "__ct__7DerivedFPc".into(),
            is_static: false,
            is_weak: true,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 24 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Char),
                    name: "payload".into(),
                },
            ],
            locals: Vec::new(),
            statements: vec![
                vptr_store("__vt__5BaseA"),
                vptr_store("__vt__5BaseB"),
                Statement::Store {
                    target: member(16),
                    value: Expression::Assign {
                        target: Box::new(member(12)),
                        value: Box::new(Expression::Assign {
                            target: Box::new(member(8)),
                            value: Box::new(Expression::IntegerLiteral(0)),
                        }),
                    },
                },
                Statement::Store {
                    target: member(4),
                    value: Expression::StringLiteral(name.clone()),
                },
                vptr_store("__vt__5BaseC"),
                Statement::Expression(Expression::Call {
                    name: "init__5BaseCFPc".into(),
                    arguments: vec![
                        Expression::Variable("this".into()),
                        Expression::StringLiteral(name),
                    ],
                }),
                vptr_store("__vt__7Derived"),
                Statement::Store {
                    target: member(20),
                    value: Expression::Variable("payload".into()),
                },
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("this".into())),
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
    fn lowers_complete_polymorphic_constructor_transaction() {
        let strings = HashMap::from([(b"boss".to_vec(), "@371".into())]);
        let output = lower(&constructor(), &strings, CompilerConfig::new(GC_1_2_5N))
            .expect("the canonical constructor chain should lower");

        assert_eq!(output.instructions.len(), 37);
        assert_eq!(output.relocations.len(), 11);
        assert_eq!(
            output.symbol_order,
            [
                "__vt__5BaseA",
                "__vt__5BaseB",
                "@371",
                "__vt__5BaseC",
                "init__5BaseCFPc",
                "__vt__7Derived",
            ]
        );
        assert!(matches!(
            &output.relocations[4].target,
            RelocationTarget::External(target) if target == "@371"
        ));
        assert!(matches!(
            &output.relocations[8].target,
            RelocationTarget::External(target) if target == "init__5BaseCFPc"
        ));
    }

    #[test]
    fn rejects_partial_chain_and_file_ipa() {
        let mut partial = constructor();
        partial.statements.pop();
        assert!(lower(&partial, &HashMap::new(), CompilerConfig::new(GC_1_2_5N)).is_none());

        let mut config = CompilerConfig::new(GC_1_2_5N);
        config.flags.ipa_file = true;
        assert!(lower(&constructor(), &HashMap::new(), config).is_none());
    }
}
