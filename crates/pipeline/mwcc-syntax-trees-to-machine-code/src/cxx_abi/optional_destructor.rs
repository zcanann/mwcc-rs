//! Lowering for an instantiated inline optional-storage destructor.
//!
//! The frontend retains the semantic shape (validity guard, nested wrapper
//! destruction, flag clear, deleting guard). CodeWarrior assigns the hidden
//! deleting flag and object pointer to a fixed saved-register schedule for this
//! shape; keeping that ABI schedule here avoids contaminating the general local
//! allocator with compiler-generated calling-convention state.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::{Behavior, CompilerConfig, FrameConvention};

struct Shape {
    valid_offset: i16,
    leaf_destructor: String,
    delete_callee: String,
}

pub(crate) fn lower(function: &Function, config: CompilerConfig) -> Option<MachineFunction> {
    if Behavior::resolve(&config).frame_convention != FrameConvention::Predecrement {
        return None;
    }
    let shape = recognize(function)?;
    let mut output = MachineFunction::new(function.name.clone());
    output.instructions = vec![
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 20 },
        Instruction::StoreWord { s: 31, a: 1, offset: 12 },
        Instruction::Or { a: 31, s: 4, b: 4 },
        Instruction::StoreWord { s: 30, a: 1, offset: 8 },
        Instruction::OrRecord { a: 30, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 22,
        },
        Instruction::LoadByteZero {
            d: 0,
            a: 30,
            offset: shape.valid_offset,
        },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 16,
        },
        Instruction::CompareLogicalWordImmediate { a: 30, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 16,
        },
        // The second nested wrapper guard reuses the preceding comparison.
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 16,
        },
        Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
        Instruction::BranchAndLink {
            target: shape.leaf_destructor.clone(),
        },
        Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        Instruction::ExtendSignHalfwordRecord { a: 0, s: 31 },
        Instruction::StoreByte {
            s: 3,
            a: 30,
            offset: shape.valid_offset,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 22,
        },
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::BranchAndLink {
            target: shape.delete_callee.clone(),
        },
        Instruction::LoadWord { d: 0, a: 1, offset: 20 },
        Instruction::Or { a: 3, s: 30, b: 30 },
        Instruction::LoadWord { d: 31, a: 1, offset: 12 },
        Instruction::LoadWord { d: 30, a: 1, offset: 8 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
        Instruction::BranchToLinkRegister,
    ];
    output.relocations = vec![
        Relocation {
            instruction_index: 15,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(shape.leaf_destructor.clone()),
        },
        Relocation {
            instruction_index: 21,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External(shape.delete_callee.clone()),
        },
    ];
    output.symbol_order = vec![shape.leaf_destructor.clone(), shape.delete_callee.clone()];
    output.referenced_function_symbols = output.symbol_order.clone();
    output.implicit_external_callees = output.symbol_order.clone();
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
    if !function.name.starts_with("__dt__")
        || function.parameters.len() != 2
        || function.parameters[0].name != "this"
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || function.parameters[1].name != "__destroy"
        || function.parameters[1].parameter_type != Type::Short
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == "this"
        )
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(object),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if object != "this" || !else_body.is_empty() {
        return None;
    }
    let [valid_guard, clear, delete_guard] = then_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Member {
                base: valid_base,
                offset: valid_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        then_body: destruction,
        else_body: valid_else,
    } = valid_guard
    else {
        return None;
    };
    if !is_this(valid_base) || !valid_else.is_empty() {
        return None;
    }
    let leaf_destructor = nested_leaf_destructor(destruction)?;
    let Statement::Store {
        target:
            Expression::Member {
                base: clear_base,
                offset: clear_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        value: Expression::IntegerLiteral(0),
    } = clear
    else {
        return None;
    };
    if !is_this(clear_base) || clear_offset != valid_offset {
        return None;
    }
    let delete_callee = deleting_callee(delete_guard)?;
    Some(Shape {
        valid_offset: i16::try_from(*valid_offset).ok()?,
        leaf_destructor,
        delete_callee,
    })
}

fn nested_leaf_destructor(statements: &[Statement]) -> Option<String> {
    let mut statements = statements;
    for _ in 0..2 {
        let [Statement::If {
            condition: Expression::Variable(object),
            then_body,
            else_body,
        }] = statements
        else {
            return None;
        };
        if object != "this" || !else_body.is_empty() {
            return None;
        }
        statements = then_body;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] = statements else {
        return None;
    };
    matches!(
        arguments.as_slice(),
        [Expression::Variable(object), Expression::IntegerLiteral(0)] if object == "this"
    )
    .then(|| name.clone())
}

fn deleting_callee(statement: &Statement) -> Option<String> {
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
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == "__destroy")
        || !matches!(right.as_ref(), Expression::IntegerLiteral(0))
        || !else_body.is_empty()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name, arguments })] = then_body.as_slice()
    else {
        return None;
    };
    matches!(arguments.as_slice(), [Expression::Variable(object)] if object == "this")
        .then(|| name.clone())
}

fn is_this(expression: &Expression) -> bool {
    matches!(expression, Expression::Variable(name) if name == "this")
}
