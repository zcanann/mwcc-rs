//! Compiler-generated framing for embedded register inline-asm wrappers.

use mwcc_machine_code::{FrameInfo, Instruction, MachineFunction};
use mwcc_syntax_trees::{AsmItem, AsmOperand, Expression, Function, Type};
use mwcc_versions::Behavior;

/// Lower the measured `register float`/`fsel` wrapper shape. The inline-asm
/// instruction itself is source-authored, but the f31 allocation and stack
/// frame are compiler products; keeping this separate from naked asm functions
/// preserves ordinary debug, symbol, and `.mwcats` behavior.
pub(crate) fn lower_register_inline_asm_wrapper(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
) -> Option<MachineFunction> {
    if function.return_type != Type::Float
        || !function.statements.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [condition, positive, negative] = function.parameters.as_slice() else {
        return None;
    };
    if [condition, positive, negative]
        .iter()
        .any(|parameter| parameter.parameter_type != Type::Float)
    {
        return None;
    }
    let [result] = function.locals.as_slice() else {
        return None;
    };
    if result.declared_type != Type::Float
        || result.initializer.is_some()
        || result.is_static
        || result.array_length.is_some()
        || !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        )
    {
        return None;
    }
    let [block] = function.inline_asm_blocks.as_slice() else {
        return None;
    };
    let [AsmItem::Instruction(instruction)] = block.items.as_slice() else {
        return None;
    };
    let [AsmOperand::Label(destination), AsmOperand::Label(condition_operand), AsmOperand::Label(positive_operand), AsmOperand::Label(negative_operand)] =
        instruction.operands.as_slice()
    else {
        return None;
    };
    if block.statement_index != 0
        || instruction.mnemonic != "fsel"
        || destination != &result.name
        || condition_operand != &condition.name
        || positive_operand != &positive.name
        || negative_operand != &negative.name
    {
        return None;
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 8,
        },
        Instruction::FloatSelect {
            d: 31,
            a: 1,
            c: 2,
            b: 3,
        },
        Instruction::FloatMove { d: 1, b: 31 },
        Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 8,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        },
        Instruction::BranchToLinkRegister,
    ]);
    output.pre_scheduled = true;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    if cpp_exceptions && behavior.emit_leaf_frame_unwind {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 0,
            saved_fpr_count: 1,
            uses_fpu: true,
        });
    }
    Some(output)
}
