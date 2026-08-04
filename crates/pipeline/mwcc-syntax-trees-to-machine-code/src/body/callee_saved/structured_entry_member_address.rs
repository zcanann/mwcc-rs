//! Entry member addresses retained through an early call-bearing repair arm.
//!
//! Some linkage-first frames load an addressable scalar local from a member of
//! an addressable parameter, then write that same member after a call-bearing
//! ownership check. MWCC gives the lvalue its own lifetime: the initial value
//! is loaded directly, while a generated address is retained for the later
//! store. Keeping that lifetime explicit lets ordinary saved-home coalescing
//! reuse its register for unrelated loop-local addresses after the repair arm.

use super::*;

pub(super) const ADDRESS_PREFIX: &str = "__mwcc_entry_member_address_";

pub(super) struct Materialization {
    pub(super) function: Function,
    pub(super) source_local: String,
    pub(super) address_local: String,
    owner: String,
    pub(super) member_offset: u32,
}

pub(super) fn materialize(function: &Function) -> Option<Materialization> {
    let passive = super::structured_passive_frame_scalar_mirrors::Plan::recognize(function)?;
    let mut candidate = None;
    for local in &function.locals {
        if !passive.contains(&local.name) {
            continue;
        }
        let Some(member @ Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        }) = local.initializer.as_ref()
        else {
            continue;
        };
        let Expression::Variable(owner) = base.as_ref() else {
            continue;
        };
        if !passive.contains(owner)
            || super::structured_expression_visit::statements_assign_name(
                &function.statements,
                owner,
            )
        {
            continue;
        }
        let Some(address_type) = address_type(*member_type) else {
            continue;
        };
        let call_bearing_use = function.statements.iter().any(|statement| {
            crate::analysis::statement_has_call(statement)
                && statement_occurrences(statement, member) != 0
        });
        if !call_bearing_use {
            continue;
        }
        if candidate.is_some() {
            return None;
        }
        candidate = Some((
            local.name.clone(),
            owner.clone(),
            member.clone(),
            *offset,
            address_type,
        ));
    }
    let (source_local, owner, member, member_offset, address_type) = candidate?;

    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let address_local = fresh_name(&mut used);
    let mut rewritten = function.clone();
    rewritten.locals.push(LocalDeclaration {
        declared_type: address_type,
        name: address_local.clone(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    });
    rewritten.statements = std::iter::once(Statement::Assign {
        name: address_local.clone(),
        value: Expression::AddressOf {
            operand: Box::new(member.clone()),
        },
    })
    .chain(function.statements.iter().map(|statement| {
        super::structured_expression_visit::rewrite_statement(statement, &mut |expression| {
            crate::analysis::structurally_equal(expression, &member).then(|| {
                Expression::Dereference {
                    pointer: Box::new(Expression::Variable(address_local.clone())),
                }
            })
        })
    }))
    .collect();

    Some(Materialization {
        function: rewritten,
        source_local,
        address_local,
        owner,
        member_offset,
    })
}

impl Materialization {
    /// MWCC overlaps lvalue formation with the stack publication of the value
    /// loaded from that lvalue. The source transformation must initially emit
    /// the address assignment after local initialization; move only its pure
    /// `addi` once the exact frame/load transaction has been verified.
    pub(super) fn schedule_entry(&self, generator: &mut Generator) -> bool {
        let Some(source_home) = generator
            .locations
            .get(&self.source_local)
            .map(|location| location.register)
        else {
            return false;
        };
        let Some(owner_home) = generator
            .locations
            .get(&self.owner)
            .map(|location| location.register)
        else {
            return false;
        };
        let Some(address_home) = generator
            .locations
            .get(&self.address_local)
            .map(|location| location.register)
        else {
            return false;
        };
        let Some(frame_offset) = generator
            .frame_slots
            .get(&self.source_local)
            .map(|slot| slot.offset)
        else {
            return false;
        };
        let Ok(member_offset) = i16::try_from(self.member_offset) else {
            return false;
        };
        let Some(start) = entry_transaction(
            &generator.output.instructions,
            source_home,
            owner_home,
            address_home,
            frame_offset,
            member_offset,
        ) else {
            return false;
        };
        generator.move_instruction_before(start + 3, start + 1);
        true
    }
}

fn entry_transaction(
    instructions: &[Instruction],
    source_home: u8,
    owner_home: u8,
    address_home: u8,
    frame_offset: i16,
    member_offset: i16,
) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: loaded, a: load_base, offset: load_offset },
            Instruction::StoreWord { s: stored, a: 1, offset: stored_offset },
            Instruction::LoadWord { d: reloaded, a: 1, offset: reloaded_offset },
            Instruction::AddImmediate { d: address, a: address_base, immediate },
        ] if loaded == stored
            && load_base == &owner_home
            && load_offset == &member_offset
            && stored_offset == &frame_offset
            && reloaded == &source_home
            && reloaded_offset == &frame_offset
            && address == &address_home
            && address_base == &owner_home
            && immediate == &member_offset)
    })
}

fn statement_occurrences(statement: &Statement, member: &Expression) -> usize {
    let mut count = 0usize;
    super::structured_expression_visit::visit_statement(statement, &mut |expression| {
        count += usize::from(crate::analysis::structurally_equal(expression, member));
    });
    count
}

fn address_type(member_type: Type) -> Option<Type> {
    Some(Type::Pointer(match member_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        Type::Double => Pointee::Double,
        Type::Pointer(_) | Type::StructPointer { .. } => Pointee::Pointer,
        Type::LongLong | Type::UnsignedLongLong | Type::Void | Type::Struct { .. } => return None,
    }))
}

fn fresh_name(used: &mut std::collections::HashSet<String>) -> String {
    for index in 0usize.. {
        let name = format!("{ADDRESS_PREFIX}{index}");
        if used.insert(name.clone()) {
            return name;
        }
    }
    unreachable!("an unbounded generated-name sequence was exhausted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Pointer(Pointee::Pointer),
            name: name.into(),
            initializer,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("owner".into())),
            offset: 8,
            member_type: Type::Pointer(Pointee::UnsignedInt),
            index_stride: None,
        }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "repair".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                name: "owner".into(),
                parameter_type: Type::StructPointer { element_size: 16 },
            }],
            locals: vec![
                local("value", Some(member())),
                local(
                    "owner_alias",
                    Some(Expression::AddressOf {
                        operand: Box::new(Expression::Variable("owner".into())),
                    }),
                ),
                local(
                    "value_alias",
                    Some(Expression::AddressOf {
                        operand: Box::new(Expression::Variable("value".into())),
                    }),
                ),
            ],
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
    fn retains_the_lvalue_but_leaves_the_initial_member_load_direct() {
        let source = function(vec![Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![
                Statement::Expression(Expression::Call {
                    name: "mutate".into(),
                    arguments: Vec::new(),
                }),
                Statement::Store {
                    target: member(),
                    value: Expression::IntegerLiteral(0),
                },
            ],
            else_body: Vec::new(),
        }]);
        let materialized = materialize(&source).expect("entry member address");

        assert!(matches!(
            source.locals[0].initializer,
            Some(Expression::Member { .. })
        ));
        assert!(matches!(
            materialized.function.locals[0].initializer,
            Some(Expression::Member { .. })
        ));
        assert!(matches!(
            materialized.function.statements.as_slice(),
            [Statement::Assign { name, value: Expression::AddressOf { .. } }, Statement::If { then_body, .. }]
                if name == &materialized.address_local
                    && matches!(then_body.last(), Some(Statement::Store {
                        target: Expression::Dereference { pointer }, ..
                    }) if matches!(pointer.as_ref(), Expression::Variable(name)
                        if name == &materialized.address_local))
        ));
    }

    #[test]
    fn rejects_a_repeated_member_without_a_call_bearing_use() {
        let source = function(vec![Statement::Store {
            target: member(),
            value: Expression::IntegerLiteral(0),
        }]);
        assert!(materialize(&source).is_none());
    }

    #[test]
    fn recognizes_the_address_after_its_frame_round_trip() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 72,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 72,
            },
            Instruction::AddImmediate {
                d: 28,
                a: 31,
                immediate: 8,
            },
        ];

        assert_eq!(entry_transaction(&instructions, 30, 31, 28, 72, 8), Some(0));
    }
}
