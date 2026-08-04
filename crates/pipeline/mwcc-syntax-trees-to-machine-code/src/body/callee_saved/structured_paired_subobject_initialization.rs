//! Initialization of two embedded subobjects followed by scalar member stores.
//!
//! Legacy MWCC retains the complete incoming-parameter table for this shape:
//! the owner pointer crosses both initializer calls and every entry value is
//! then committed to the owner. The zero materialization also fills the issue
//! slot between the first two scalar stores.

use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Function, Statement, Type};

#[derive(Clone, Copy)]
pub(super) struct StructuredPairedSubobjectInitialization;

impl StructuredPairedSubobjectInitialization {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        if function.return_type != Type::Void
            || function.parameters.len() != 3
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return None;
        }
        let [first_call, second_call, first_store, second_store, first_zero, second_zero] =
            function.statements.as_slice()
        else {
            return None;
        };
        let owner = &function.parameters[0].name;
        let first_value = &function.parameters[1].name;
        let second_value = &function.parameters[2].name;
        let (first_callee, first_offset) = member_initializer_call(first_call, owner)?;
        let (second_callee, second_offset) = member_initializer_call(second_call, owner)?;
        if first_callee != second_callee || (first_offset, second_offset) != (0, 8) {
            return None;
        }
        if !member_variable_store(first_store, owner, first_value, 16)
            || !member_variable_store(second_store, owner, second_value, 20)
            || !member_zero_store(first_zero, owner, 24)
            || !member_zero_store(second_zero, owner, 28)
        {
            return None;
        }
        Some(Self)
    }

    pub(super) fn schedule(self, instructions: &mut [Instruction]) {
        let Some(start) = instructions.windows(5).position(|window| {
            matches!(window, [
                Instruction::StoreWord { a: first_base, offset: 16, .. },
                Instruction::StoreWord { a: second_base, offset: 20, .. },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
                Instruction::StoreWord { s: 0, a: first_zero_base, offset: 24 },
                Instruction::StoreWord { s: 0, a: second_zero_base, offset: 28 },
            ] if first_base == second_base
                && first_base == first_zero_base
                && first_base == second_zero_base)
        }) else {
            return;
        };
        instructions.swap(start + 1, start + 2);
    }
}

fn member_initializer_call<'a>(statement: &'a Statement, owner: &str) -> Option<(&'a str, u32)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [Expression::AddressOf { operand }] = arguments.as_slice() else {
        return None;
    };
    let Expression::Member { base, offset, .. } = operand.as_ref() else {
        return None;
    };
    variable(base, owner).then_some((name.as_str(), *offset))
}

fn member_variable_store(
    statement: &Statement,
    owner: &str,
    value_name: &str,
    expected_offset: u32,
) -> bool {
    let Statement::Store { target, value } = statement else {
        return false;
    };
    member(target, owner, expected_offset) && variable(value, value_name)
}

fn member_zero_store(statement: &Statement, owner: &str, expected_offset: u32) -> bool {
    let Statement::Store { target, value } = statement else {
        return false;
    };
    member(target, owner, expected_offset)
        && matches!(value, Expression::IntegerLiteral(0))
}

fn member(expression: &Expression, owner: &str, expected_offset: u32) -> bool {
    matches!(expression,
        Expression::Member { base, offset, .. }
            if *offset == expected_offset && variable(base, owner))
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Parameter, Pointee};

    fn member(owner: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(owner.into())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn function() -> Function {
        let call = |offset| Statement::Expression(Expression::Call {
            name: "initialize".into(),
            arguments: vec![Expression::AddressOf {
                operand: Box::new(member("owner", offset)),
            }],
        });
        let store = |offset, value| Statement::Store {
            target: member("owner", offset),
            value,
        };
        Function {
            return_type: Type::Void,
            name: "initialize_owner".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter { name: "owner".into(), parameter_type: Type::Pointer(Pointee::Int) },
                Parameter { name: "buffer".into(), parameter_type: Type::Pointer(Pointee::Int) },
                Parameter { name: "count".into(), parameter_type: Type::Int },
            ],
            locals: Vec::new(),
            statements: vec![
                call(0),
                call(8),
                store(16, Expression::Variable("buffer".into())),
                store(20, Expression::Variable("count".into())),
                store(24, Expression::IntegerLiteral(0)),
                store(28, Expression::IntegerLiteral(0)),
            ],
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
    fn recognizes_the_complete_paired_initialization_transaction() {
        let function = function();
        assert!(StructuredPairedSubobjectInitialization::plan(&function).is_some());

        let mut incomplete = function;
        incomplete.statements.pop();
        assert!(StructuredPairedSubobjectInitialization::plan(&incomplete).is_none());
    }

    #[test]
    fn schedules_zero_materialization_between_scalar_stores() {
        let mut instructions = vec![
            Instruction::StoreWord { s: 30, a: 29, offset: 16 },
            Instruction::StoreWord { s: 31, a: 29, offset: 20 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 29, offset: 24 },
            Instruction::StoreWord { s: 0, a: 29, offset: 28 },
        ];

        StructuredPairedSubobjectInitialization.schedule(&mut instructions);

        assert!(matches!(instructions[1], Instruction::AddImmediate { d: 0, a: 0, immediate: 0 }));
        assert!(matches!(instructions[2], Instruction::StoreWord { offset: 20, .. }));
    }
}
