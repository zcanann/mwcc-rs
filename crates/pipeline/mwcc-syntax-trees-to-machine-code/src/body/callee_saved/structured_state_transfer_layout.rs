//! Layout and entry scheduling for object-state transfers.
//!
//! These functions retain a source object, a call-produced destination object,
//! and its payload alongside both incoming parameters. A dead source scratch
//! array has no runtime frame lane; the five values occupy r31 through r27 and
//! are saved as one dense range.

#[allow(unused_imports)]
use super::*;

pub(super) fn is_unused_array_state_transfer(function: &Function) -> bool {
    let [receiver, callback] = function.parameters.as_slice() else {
        return false;
    };
    let [source, destination_object, destination, ..] = function.locals.as_slice() else {
        return false;
    };
    let Some(Expression::Member {
        base: source_base,
        member_type: Type::Pointer(_),
        ..
    }) = source.initializer.as_ref()
    else {
        return false;
    };
    let Some(Expression::Call {
        arguments: destination_arguments,
        ..
    }) = destination_object.initializer.as_ref()
    else {
        return false;
    };
    let Some(Expression::Member {
        base: destination_base,
        member_type: Type::Pointer(_),
        ..
    }) = destination.initializer.as_ref()
    else {
        return false;
    };
    matches!(
        source_base.as_ref(),
        Expression::Variable(name) if name == &receiver.name
    ) && matches!(
        destination_arguments.as_slice(),
        [
            Expression::Member { base, .. },
            Expression::IntegerLiteral(1),
        ] if matches!(base.as_ref(), Expression::Variable(name) if name == &source.name)
    ) && matches!(
        destination_base.as_ref(),
        Expression::Variable(name) if name == &destination_object.name
    ) && function
        .statements
        .last()
        .is_some_and(|statement| statement_references_name(statement, &callback.name))
}

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_entry(&mut self, function: &Function) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(start) = allocated_state_transfer_entry(&self.output.instructions) else {
            return;
        };

        self.move_instruction_before(start + 1, start);
        crate::insert_instruction_retargeting(self, start + 1, Instruction::move_register(27, 3));
        self.move_instruction_before(start + 3, start + 2);

        crate::remove_instruction_retargeting_to_next(self, start + 9);
        crate::remove_instruction_retargeting_to_next(self, start + 8);
        self.move_instruction_before(start + 9, start + 7);
        self.output.instructions[start + 6] = Instruction::move_register(30, 3);
    }
}

fn allocated_state_transfer_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions
        .windows(15)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 31, a: 3, .. },
                    Instruction::AddImmediate {
                        d: 28,
                        a: 4,
                        immediate: 0,
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: 0,
                        immediate: 1,
                    },
                    Instruction::LoadByteZero { d: 3, a: 31, .. },
                    Instruction::BranchAndLink { .. },
                    Instruction::AddImmediate {
                        d: 30,
                        a: 3,
                        immediate: 0,
                    },
                    Instruction::LoadWord { d: 29, a: 30, .. },
                    Instruction::AddImmediate {
                        d: 28,
                        a: 4,
                        immediate: 0,
                    },
                    Instruction::AddImmediate {
                        d: 27,
                        a: 3,
                        immediate: 0,
                    },
                    Instruction::LoadByteZero { d: 3, a: 31, .. },
                    Instruction::LoadByteZero { d: 4, a: 31, .. },
                    Instruction::RotateAndMask { a: 4, s: 4, .. },
                    Instruction::LoadByteZero { d: 5, a: 29, .. },
                    Instruction::RotateAndMask { a: 5, s: 5, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
            .then_some(start)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unrelated_short_entry() {
        assert_eq!(
            allocated_state_transfer_entry(&[
                Instruction::LoadWord {
                    d: 31,
                    a: 3,
                    offset: 44,
                },
                Instruction::move_register(28, 4),
            ]),
            None
        );
    }
}
