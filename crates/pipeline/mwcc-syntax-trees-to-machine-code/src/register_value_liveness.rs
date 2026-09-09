//! Conservative lifetime proof shared by register-value reuse passes.
use mwcc_machine_code::Instruction;

// A removed temporary definition must be dead along every successor path.
// Stop at a new definition; do not infer targets for an indirect branch.
pub(crate) fn dead_after(instructions: &[Instruction], start: usize, register: u32) -> bool {
    use mwcc_vreg::{Class, RegisterRole};
    if register == 3 {
        return false;
    }
    let mut pending = vec![start];
    let mut visited = std::collections::HashSet::new();
    while let Some(at) = pending.pop() {
        if at >= instructions.len() || !visited.insert(at) {
            continue;
        }
        let instruction = &instructions[at];
        let operands = mwcc_vreg::register_operands(instruction);
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Use
        }) {
            return false;
        }
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Define
        }) {
            continue;
        }
        match instruction {
            Instruction::Branch { target } => pending.push(*target),
            Instruction::BranchConditionalForward { target, .. } => {
                pending.push(*target);
                pending.push(at + 1);
            }
            Instruction::BranchToLinkRegister => {}
            Instruction::BranchToCountRegister
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::BranchToLinkRegisterAndLink
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchImmediate { .. }
            | Instruction::ReturnFromInterrupt
            | Instruction::SystemCall
            | Instruction::VerbatimWord(_)
            | Instruction::BranchExternal { .. } => return false,
            _ => pending.push(at + 1),
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_successors_and_opaque_instructions_cannot_prove_a_value_dead() {
        for instruction in [
            Instruction::BranchImmediate {
                value: 32,
                absolute: false,
                link: false,
            },
            Instruction::ReturnFromInterrupt,
            Instruction::SystemCall,
            Instruction::VerbatimWord(0x4e800020),
        ] {
            assert!(!dead_after(&[instruction], 0, 4));
        }
        assert!(dead_after(&[Instruction::BranchToLinkRegister], 0, 4));
        assert!(!dead_after(&[Instruction::BranchToLinkRegister], 0, 3));
    }
}
