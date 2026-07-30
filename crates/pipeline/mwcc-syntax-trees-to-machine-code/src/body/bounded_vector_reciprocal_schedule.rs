//! Late schedule for a bounded scale followed by reciprocal vector scaling.
//!
//! Structured lowering retains a select temporary and reloads the coefficient
//! table. MWCC writes the selected bound directly to the saved float, keeps one
//! table base live, and overlaps linkage setup with the integer conversion.

#[allow(unused_imports)]
use super::*;

struct Shape {
    object_offset: i16,
    integer_offset: i16,
    vector_offset: i16,
    item_offset: i16,
    scale_offset: i16,
    coefficient_offset: i16,
    intercept_offset: i16,
    bound_offset: i16,
    first_call: String,
    second_call: String,
}

impl Generator {
    pub(crate) fn schedule_bounded_vector_reciprocal(&mut self) {
        let Some((start, shape)) = self
            .output
            .instructions
            .windows(60)
            .enumerate()
            .find_map(|(start, window)| recognize(window).map(|shape| (start, shape)))
        else {
            return;
        };
        if !expected_relocations(self, start) {
            return;
        }

        for relative in [26, 22, 20] {
            crate::remove_instruction_retargeting_to_next(self, start + relative);
        }

        self.output.instructions[start..start + 57].clone_from_slice(&scheduled(&shape, start));
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 12 => start + 11,
                index if index == start + 16 => start + 8,
                index => index,
            };
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 60).contains(&relocation.instruction_index))
        .map(|relocation| relocation.instruction_index - start)
        .collect::<Vec<_>>();
    relative == [12, 16, 20, 27, 46, 52]
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 16,
            start + 20,
        )
}

fn recognize(window: &[Instruction]) -> Option<Shape> {
    let [Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 4,
    }, Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: -56,
    }, Instruction::StoreFloatDouble {
        s: 31,
        a: 1,
        offset: 48,
    }, Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 44,
    }, Instruction::StoreWord {
        s: 30,
        a: 1,
        offset: 40,
    }, Instruction::LoadWord {
        d: 30,
        a: 3,
        offset: object_offset,
    }, Instruction::AddImmediate {
        d: 31,
        a: 30,
        immediate: vector_offset,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: integer_offset,
    }, Instruction::XorImmediateShifted {
        a: 0,
        s: 3,
        immediate: 0x8000,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 12,
    }, Instruction::AddImmediateShifted {
        d: 0,
        a: 0,
        immediate: 0x4330,
    }, Instruction::LoadFloatDouble {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 8,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 1,
        offset: 8,
    }, Instruction::FloatSubtractSingle { d: 2, a: 0, b: 3 }, Instruction::LoadWord { d: 3, a: 0, .. }, Instruction::LoadFloatSingle {
        d: 1,
        a: 3,
        offset: coefficient_offset,
    }, Instruction::LoadFloatSingle {
        d: 31,
        a: 3,
        offset: intercept_offset,
    }, Instruction::FloatMultiplyAddSingle {
        d: 31,
        a: 2,
        c: 1,
        b: 31,
    }, Instruction::LoadWord { d: 3, a: 0, .. }, Instruction::LoadFloatSingle {
        d: 1,
        a: 3,
        offset: bound_offset,
    }, Instruction::FloatMove { d: 2, b: 31 }, Instruction::FloatCompareOrdered { a: 31, b: 1 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 1,
        ..
    }, Instruction::FloatMove { d: 2, b: 1 }, Instruction::FloatMove { d: 31, b: 2 }, Instruction::LoadFloatSingle { d: 0, a: 0, .. }, Instruction::FloatDivideSingle { d: 1, a: 0, b: 31 }, Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: 0,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 28,
    }, Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: 4,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 32,
    }, Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: 8,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 36,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 1,
        offset: 28,
    }, Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 28,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 1,
        offset: 32,
    }, Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 32,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 1,
        offset: 36,
    }, Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 36,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: item_offset,
    }, Instruction::AddImmediate {
        d: 4,
        a: 1,
        immediate: 28,
    }, Instruction::BranchAndLink { target: first_call }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: second_item_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 31,
        offset: 12,
    }, Instruction::FloatMultiplySingle { d: 1, a: 31, c: 0 }, Instruction::LoadFloatSingle {
        d: 0,
        a: 30,
        offset: scale_offset,
    }, Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 }, Instruction::BranchAndLink {
        target: second_call,
    }, Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 60,
    }, Instruction::LoadFloatDouble {
        d: 31,
        a: 1,
        offset: 48,
    }, Instruction::LoadWord {
        d: 31,
        a: 1,
        offset: 44,
    }, Instruction::LoadWord {
        d: 30,
        a: 1,
        offset: 40,
    }, Instruction::AddImmediate {
        d: 1,
        a: 1,
        immediate: 56,
    }, Instruction::MoveToLinkRegister { s: 0 }, Instruction::BranchToLinkRegister] = window
    else {
        return None;
    };
    (item_offset == second_item_offset
        && intercept_offset + 4 == *bound_offset
        && bound_offset + 4 == *coefficient_offset)
        .then(|| Shape {
            object_offset: *object_offset,
            integer_offset: *integer_offset,
            vector_offset: *vector_offset,
            item_offset: *item_offset,
            scale_offset: *scale_offset,
            coefficient_offset: *coefficient_offset,
            intercept_offset: *intercept_offset,
            bound_offset: *bound_offset,
            first_call: first_call.clone(),
            second_call: second_call.clone(),
        })
}

fn scheduled(shape: &Shape, start: usize) -> [Instruction; 57] {
    [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::AddImmediateShifted {
            d: 0,
            a: 0,
            immediate: 0x4330,
        },
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -72,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 64,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 60,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 56,
        },
        Instruction::LoadWord {
            d: 31,
            a: 3,
            offset: shape.object_offset,
        },
        Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.integer_offset,
        },
        Instruction::AddImmediate {
            d: 30,
            a: 31,
            immediate: shape.vector_offset,
        },
        Instruction::LoadFloatDouble {
            d: 2,
            a: 0,
            offset: 0,
        },
        Instruction::XorImmediateShifted {
            a: 3,
            s: 3,
            immediate: 0x8000,
        },
        Instruction::LoadFloatSingle {
            d: 3,
            a: 4,
            offset: shape.coefficient_offset,
        },
        Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 52,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: shape.intercept_offset,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 48,
        },
        Instruction::LoadFloatSingle {
            d: 4,
            a: 4,
            offset: shape.bound_offset,
        },
        Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 48,
        },
        Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 },
        Instruction::FloatMultiplyAddSingle {
            d: 31,
            a: 3,
            c: 1,
            b: 0,
        },
        Instruction::FloatCompareOrdered { a: 31, b: 4 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: start + 24,
        },
        Instruction::FloatMove { d: 31, b: 4 },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 32,
        },
        Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: 0,
        },
        Instruction::FloatDivideSingle { d: 1, a: 0, b: 31 },
        Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 4,
        },
        Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 32,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        },
        Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: 8,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 40,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 32,
        },
        Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 1,
            offset: 32,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 36,
        },
        Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 1,
            offset: 36,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 40,
        },
        Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 1,
            offset: 40,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::BranchAndLink {
            target: shape.first_call.clone(),
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 30,
            offset: 12,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 31,
            offset: shape.scale_offset,
        },
        Instruction::FloatMultiplySingle { d: 0, a: 31, c: 0 },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
        Instruction::BranchAndLink {
            target: shape.second_call.clone(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 76,
        },
        Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 64,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 60,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 56,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 72,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unrelated_short_stream() {
        assert!(recognize(&vec![Instruction::BranchToLinkRegister; 60]).is_none());
    }
}
