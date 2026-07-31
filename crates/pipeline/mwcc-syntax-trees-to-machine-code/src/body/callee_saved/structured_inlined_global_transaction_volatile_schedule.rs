//! Volatile scratch reuse after a composed scalar transaction.
//!
//! Physical liveness conservatively pins r5 across a loop containing several
//! absolute accesses to one mutable global. MWCC ends that lifetime at the
//! transaction's terminal load, then reuses r5 for the paired conversion
//! constants and the indexed swap value. This finalizer runs after allocation,
//! when the complete measured topology can prove that r6 is only the
//! allocator's substitute for that non-overlapping r5 lifetime.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_inlined_global_transaction_volatile_reuse(&mut self) {
        if self.inline_global_transaction_result_homes.len() != 2 {
            return;
        }
        let Some(start) = self.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::Or {
                    a: 27,
                    s: 28,
                    b: 28
                }
            )
        }) else {
            return;
        };
        if !self.output.instructions[..start].iter().any(|instruction| {
            matches!(instruction, Instruction::BranchAndLink { target } if target == "_savegpr_27")
        }) {
            return;
        }
        let end = self.output.instructions[start + 1..]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::BranchAndLink { target } if target.starts_with("_restgpr_"))
            })
            .map(|offset| start + 1 + offset)
            .unwrap_or(self.output.instructions.len());
        let region = &self.output.instructions[start + 1..end];
        if region.iter().any(|instruction| {
            mwcc_vreg::register_operands(instruction)
                .iter()
                .any(|operand| operand.class == mwcc_vreg::Class::General && operand.register == 5)
        }) {
            return;
        }
        let rewritable = region
            .iter()
            .filter(|instruction| touches_general_register(instruction, 6))
            .all(|instruction| is_transaction_r6_use(instruction));
        let touched = region
            .iter()
            .filter(|instruction| touches_general_register(instruction, 6))
            .count();
        if !rewritable || touched != 11 {
            return;
        }
        for instruction in &mut self.output.instructions[start + 1..end] {
            rewrite_transaction_r6(instruction);
        }
    }
}

fn touches_general_register(instruction: &Instruction, register: u8) -> bool {
    mwcc_vreg::register_operands(instruction)
        .iter()
        .any(|operand| operand.class == mwcc_vreg::Class::General && operand.register == register)
}

fn is_transaction_r6_use(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediateShifted { d: 6, a: 0, .. }
            | Instruction::AddImmediate { d: 6, a: 6, .. }
            | Instruction::LoadFloatDouble { a: 6, .. }
            | Instruction::LoadFloatSingle { a: 6, .. }
            | Instruction::LoadWordIndexed { d: 6, .. }
            | Instruction::StoreWordIndexed { s: 6, .. }
    )
}

fn rewrite_transaction_r6(instruction: &mut Instruction) {
    match instruction {
        Instruction::AddImmediateShifted { d, a: 0, .. } if *d == 6 => *d = 5,
        Instruction::AddImmediate { d, a, .. } if *d == 6 && *a == 6 => {
            *d = 5;
            *a = 5;
        }
        Instruction::LoadFloatDouble { a, .. } if *a == 6 => *a = 5,
        Instruction::LoadFloatSingle { a, .. } if *a == 6 => *a = 5,
        Instruction::LoadWordIndexed { d, .. } if *d == 6 => *d = 5,
        Instruction::StoreWordIndexed { s, .. } if *s == 6 => *s = 5,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_the_transaction_scratch_vocabulary() {
        let mut instructions = vec![
            Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 6,
                immediate: 0,
            },
            Instruction::LoadFloatDouble {
                d: 1,
                a: 6,
                offset: 0,
            },
            Instruction::LoadWordIndexed { d: 6, a: 3, b: 0 },
            Instruction::StoreWordIndexed { s: 6, a: 3, b: 0 },
        ];
        assert!(instructions.iter().all(is_transaction_r6_use));
        for instruction in &mut instructions {
            rewrite_transaction_r6(instruction);
        }
        assert!(instructions
            .iter()
            .all(|instruction| !touches_general_register(instruction, 6)));
        assert!(instructions
            .iter()
            .all(|instruction| touches_general_register(instruction, 5)));
    }
}
