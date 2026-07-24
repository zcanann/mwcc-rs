//! Repeated two-member call arguments sharing one retained base.
//!
//! Build 163 loads the second argument first when both are byte members of the
//! same callee-saved object and the second feeds a bit-field extraction. This
//! pass verifies both complete call prefixes before changing either one.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_shared_member_arguments(&mut self, function: &Function) {
        let Some(call_names) = recognize(function) else {
            return;
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let mut pairs = Vec::new();
        for call_name in call_names {
            let Some(call) = self.output.instructions.iter().position(|instruction| {
                matches!(instruction, Instruction::BranchAndLink { target } if target == call_name)
            }) else {
                return;
            };
            let Some(first) = call.checked_sub(3) else {
                return;
            };
            if !matches!(
                &self.output.instructions[first..call],
                [
                    Instruction::LoadByteZero { d: 3, a: first_base, .. },
                    Instruction::LoadByteZero { d: 4, a: second_base, .. },
                    Instruction::RotateAndMask { a: 4, s: 4, .. },
                ] if first_base == second_base
            ) || self.output.relocations.iter().any(|relocation| {
                relocation.instruction_index == first
                    || relocation.instruction_index == first + 1
            }) {
                return;
            }
            pairs.push(first);
        }
        for first in pairs {
            self.output.instructions.swap(first, first + 1);
            self.labels.moved_before(first + 1, first);
        }
    }
}

fn recognize(function: &Function) -> Option<[&str; 2]> {
    let [alias] = function.locals.as_slice() else {
        return None;
    };
    let [
        Statement::Store { .. },
        Statement::If {
            condition: Expression::Call {
                name: first_call,
                arguments: first_arguments,
            },
            ..
        },
        Statement::Expression(Expression::Call {
            name: second_call,
            arguments: second_arguments,
        }),
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if shared_byte_and_bitfield_arguments(first_arguments, &alias.name)
        && shared_byte_and_bitfield_arguments(second_arguments, &alias.name)
    {
        Some([first_call, second_call])
    } else {
        None
    }
}

fn shared_byte_and_bitfield_arguments(arguments: &[Expression], alias: &str) -> bool {
    let [
        Expression::Member {
            base: first_base,
            member_type: Type::UnsignedChar,
            index_stride: None,
            ..
        },
        Expression::BitFieldRead { storage, .. },
    ] = arguments
    else {
        return false;
    };
    let Expression::Member {
        base: second_base,
        member_type: Type::UnsignedChar,
        index_stride: None,
        ..
    } = storage.as_ref()
    else {
        return false;
    };
    matches!(first_base.as_ref(), Expression::Variable(name) if name == alias)
        && matches!(second_base.as_ref(), Expression::Variable(name) if name == alias)
}
