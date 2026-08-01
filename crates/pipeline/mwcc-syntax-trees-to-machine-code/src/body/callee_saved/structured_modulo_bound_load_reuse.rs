//! Reuse a divisor member for the immediately following modulo-bound test.
//!
//! `(x % object.count) == object.count - 1` keeps the first count load live
//! through the divide/multiply/subtract remainder expansion. The arithmetic
//! neither changes the address nor the divisor register, so MWCC omits the
//! second member load.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_structured_modulo_bound_loads(&mut self) {
        while let Some(reload) = modulo_bound_reload(&self.output.instructions) {
            crate::remove_instruction_retargeting_to_next(self, reload);
        }
    }
}

fn modulo_bound_reload(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: divisor, a: base, offset },
            Instruction::DivideWordUnsigned {
                d: quotient,
                a: dividend,
                b: divided_by,
            },
            Instruction::MultiplyLow {
                d: product,
                a: multiplied_quotient,
                b: multiplied_divisor,
            },
            Instruction::SubtractFrom {
                d: _,
                a: subtracted_product,
                b: subtracted_from,
            },
            Instruction::LoadWord {
                d: reloaded,
                a: reload_base,
                offset: reload_offset,
            },
            Instruction::AddImmediate {
                a: bound,
                immediate: -1,
                ..
            },
        ] = window
        else {
            return None;
        };
        (*divisor == *divided_by
            && *divisor == *multiplied_divisor
            && *divisor == *reloaded
            && *divisor == *bound
            && *quotient == *multiplied_quotient
            && *product == *subtracted_product
            && *dividend == *subtracted_from
            && *base == *reload_base
            && *offset == *reload_offset)
            .then_some(start + 4)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_divisor_reloaded_for_minus_one() {
        let instructions = vec![
            Instruction::LoadWord { d: 4, a: 31, offset: 80 },
            Instruction::DivideWordUnsigned { d: 0, a: 3, b: 4 },
            Instruction::MultiplyLow { d: 0, a: 0, b: 4 },
            Instruction::SubtractFrom { d: 3, a: 0, b: 3 },
            Instruction::LoadWord { d: 4, a: 31, offset: 80 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: -1 },
        ];

        assert_eq!(modulo_bound_reload(&instructions), Some(4));
    }
}
