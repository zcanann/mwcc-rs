//! Source-proven frame and final physical schedule for buffered memory transfers.
//!
//! The frontend exposes the transfer request as five packed address-taken
//! scalars beside a large byte array. Build 159 keeps the request owner in r31
//! and the evolving status in r30, then retains the compact source error switch
//! as a jump table. The general allocator reaches the same semantic program
//! with the owner in r30 and transient results in r3; this owner performs the
//! final, topology-checked physical coloring and packet order.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) fn is_memory_transfer_frame(
    function: &Function,
    frame_arrays: &[&LocalDeclaration],
    frame_scalar_locals: &[&LocalDeclaration],
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
) -> bool {
    is_memory_transfer_frame_with_direction(
        function,
        frame_arrays,
        frame_scalar_locals,
        saved_parameters,
        deferred_saved_locals,
        1,
    )
}

pub(super) fn is_memory_write_frame(
    function: &Function,
    frame_arrays: &[&LocalDeclaration],
    frame_scalar_locals: &[&LocalDeclaration],
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
) -> bool {
    is_memory_transfer_frame_with_direction(
        function,
        frame_arrays,
        frame_scalar_locals,
        saved_parameters,
        deferred_saved_locals,
        0,
    )
}

fn is_memory_transfer_frame_with_direction(
    function: &Function,
    frame_arrays: &[&LocalDeclaration],
    frame_scalar_locals: &[&LocalDeclaration],
    saved_parameters: &[&Parameter],
    deferred_saved_locals: &[&LocalDeclaration],
    direction: i64,
) -> bool {
    if frame_arrays.len() != 1
        || frame_arrays[0].declared_type != Type::UnsignedChar
        || frame_arrays[0].array_length != Some(2048)
        || saved_parameters.len() != 1
        || deferred_saved_locals.len() != 1
        || deferred_saved_locals[0].declared_type != Type::Int
    {
        return false;
    }
    let mut scalar_types = frame_scalar_locals
        .iter()
        .map(|local| local.declared_type)
        .collect::<Vec<_>>();
    scalar_types.sort_by_key(|value_type| value_type.width());
    if scalar_types
        != [
            Type::UnsignedChar,
            Type::UnsignedChar,
            Type::UnsignedShort,
            Type::UnsignedInt,
            Type::UnsignedInt,
        ]
    {
        return false;
    }

    let transfer_buffer = &frame_arrays[0].name;
    let mut reads_into_transfer_buffer = false;
    for statement in &function.statements {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            let Expression::Call { arguments, .. } = expression else {
                return;
            };
            let [
                Expression::Variable(buffer),
                _,
                Expression::AddressOf { .. },
                _,
                Expression::IntegerLiteral(transfer_direction),
            ] = arguments.as_slice()
            else {
                return;
            };
            reads_into_transfer_buffer |=
                buffer == transfer_buffer && *transfer_direction == direction;
        });
    }

    reads_into_transfer_buffer
        && compact_error_dispatch_container_label_count(function).is_some()
}

/// Hidden labels belonging to the top-level container of the compact error
/// table. A buffered write allocates its nested table before these labels, so
/// its physical scheduler moves this amount behind the table in the ordinal
/// walk. Returning `Some(0)` distinguishes a top-level table from no table.
pub(super) fn compact_error_dispatch_container_label_count(
    function: &Function,
) -> Option<u32> {
    function.statements.iter().find_map(|statement| {
        contains_compact_error_dispatch(std::slice::from_ref(statement)).then(|| {
            super::structured::structured_hidden_label_count(std::slice::from_ref(statement))
        })
    })
}

fn contains_compact_error_dispatch(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            contains_compact_error_dispatch(then_body)
                || contains_compact_error_dispatch(else_body)
        }
        Statement::Loop { body, .. } => contains_compact_error_dispatch(body),
        Statement::Switch { arms, default, .. } => {
            let minimum = arms.iter().map(|arm| arm.value).min();
            let maximum = arms.iter().map(|arm| arm.value).max();
            arms.len() == 5
                && default.is_some()
                && minimum.zip(maximum).is_some_and(|(minimum, maximum)| {
                    maximum.checked_sub(minimum) == Some(6)
                })
        }
        _ => false,
    })
}

impl Generator {
    pub(crate) fn finalize_structured_memory_transfer_frame(&mut self) {
        if !self.structured_memory_transfer_frame {
            return;
        }
        let original = self.clone();
        if !self.try_finalize_structured_memory_transfer_frame() {
            *self = original;
        }
    }

    fn try_finalize_structured_memory_transfer_frame(&mut self) -> bool {
        let Some((frame, epilogue)) = allocated_transfer_frame(&self.output.instructions) else {
            return false;
        };

        let owner = 30;
        let result = 30;
        let owner_home = 31;
        self.output.instructions[frame + 2] = Instruction::Or {
            a: owner_home,
            s: 3,
            b: 3,
        };
        self.output.instructions[frame + 3] = Instruction::StoreWord {
            s: result,
            a: 1,
            offset: self.frame_size - 8,
        };
        for instruction in &mut self.output.instructions[frame + 4..epilogue] {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class == mwcc_vreg::Class::General && *register == owner {
                    *register = owner_home;
                }
            });
        }

        if !schedule_target_access_packet(&mut self.output.instructions) {
            return false;
        }
        canonicalize_unsigned_transfer_bound(&mut self.output.instructions);

        if !retain_initial_results(self, result) {
            return false;
        }

        if let Some(compare) = self.output.instructions.windows(4).position(|window| {
            matches!(window[0], Instruction::AddImmediate { d: 4, a: 0, immediate: 128 })
                && matches!(window[1], Instruction::AddImmediate { d: 5, a: 0, immediate: 0 })
                && matches!(window[2], Instruction::BranchAndLink { .. })
                && matches!(window[3], Instruction::CompareWordImmediate { a: 3, immediate: 0 })
        }).map(|start| start + 3)
        {
            self.output.instructions[compare] = Instruction::CompareWordImmediate {
                a: result,
                immediate: 0,
            };
        } else {
            return false;
        }

        let Some(target_call) = target_access_call(&self.output.instructions) else {
            return false;
        };
        let target_store = self.output.instructions[target_call + 2].clone();
        if !matches!(target_store, Instruction::StoreHalfword { a: 1, .. })
            || !matches!(self.output.instructions[target_call + 1], Instruction::LoadWord { a: 1, .. })
            || !matches!(self.output.instructions[target_call + 3], Instruction::CompareWordImmediate { a: 3, immediate: 0 })
        {
            return false;
        }
        self.output.instructions[target_call + 2] = Instruction::OrRecord {
            a: result,
            s: 3,
            b: 3,
        };
        self.output.instructions[target_call + 3] = target_store;

        let Some(dispatch) = dense_error_dispatch(&self.output.instructions) else {
            return false;
        };
        let append_calls = self.output.instructions[target_call + 1..dispatch]
            .iter()
            .enumerate()
            .filter_map(|(offset, instruction)| {
                matches!(instruction, Instruction::BranchAndLink { .. })
                    .then_some(target_call + 1 + offset)
            })
            .collect::<Vec<_>>();
        if append_calls.len() != 2 {
            return false;
        }
        for call in append_calls.into_iter().rev() {
            crate::insert_instruction_retargeting(
                self,
                call + 1,
                Instruction::Or {
                    a: result,
                    s: 3,
                    b: 3,
                },
            );
            let Instruction::CompareWordImmediate { a, immediate: 0 } =
                &mut self.output.instructions[call + 2]
            else {
                return false;
            };
            *a = result;
        }

        let Some(dispatch) = dense_error_dispatch(&self.output.instructions) else {
            return false;
        };
        if let Instruction::CompareWordImmediate { a, immediate: 0 } =
            &mut self.output.instructions[dispatch - 2]
        {
            *a = result;
        }
        let Instruction::AddImmediate { a, immediate: -1792, .. } =
            &mut self.output.instructions[dispatch]
        else {
            return false;
        };
        *a = result;

        if !schedule_append_buffer_packet(&mut self.output.instructions, owner_home) {
            return false;
        }
        canonicalize_owner_copies(&mut self.output.instructions, owner_home);
        true
    }
}

pub(super) fn allocated_transfer_frame(instructions: &[Instruction]) -> Option<(usize, usize)> {
    let frame = instructions.windows(4).position(|window| {
        matches!(window[0], Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
            && matches!(window[1], Instruction::StoreWord { s: 31, a: 1, .. })
            && matches!(window[2], Instruction::StoreWord { s: 30, a: 1, .. })
            && matches!(window[3], Instruction::Or { a: 30, s: 3, b: 3 })
    })?;
    let epilogue = instructions.windows(3).rposition(|window| {
        matches!(window[0], Instruction::LoadWord { d: 31, a: 1, .. })
            && matches!(window[1], Instruction::LoadWord { d: 30, a: 1, .. })
            && matches!(window[2], Instruction::AddImmediate { d: 1, a: 1, .. })
    })?;
    (frame < epilogue).then_some((frame, epilogue))
}

fn initial_output_calls(instructions: &[Instruction]) -> Option<Vec<usize>> {
    let options = instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::LoadByteZero { d: 0, a: 1, .. })
    })?;
    let calls = instructions[..options]
        .windows(2)
        .enumerate()
        .filter_map(|(start, window)| {
            (matches!(window[0], Instruction::AddImmediate { d: 4, a: 1, .. })
                && matches!(window[1], Instruction::BranchAndLink { .. }))
            .then_some(start + 1)
        })
        .collect::<Vec<_>>();
    (calls.len() == 4).then_some(calls)
}

pub(super) fn retain_initial_results(generator: &mut Generator, result: u8) -> bool {
    let Some(initial_calls) = initial_output_calls(&generator.output.instructions) else {
        return false;
    };
    for (ordinal, call) in initial_calls.into_iter().enumerate().rev() {
        if ordinal == 0 {
            generator.output.instructions[call + 1] = Instruction::OrRecord {
                a: result,
                s: 3,
                b: 3,
            };
        } else {
            crate::insert_instruction_retargeting(
                generator,
                call + 1,
                Instruction::Or {
                    a: result,
                    s: 3,
                    b: 3,
                },
            );
            if ordinal < 3 {
                let Instruction::CompareWordImmediate { a, immediate: 0 } =
                    &mut generator.output.instructions[call + 2]
                else {
                    return false;
                };
                *a = result;
            }
        }
    }
    true
}

fn target_access_call(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(2).position(|window| {
        matches!(window[0], Instruction::AddImmediate { d: 7, a: 0, immediate: 1 })
            && matches!(window[1], Instruction::BranchAndLink { .. })
    }).map(|start| start + 1)
}

pub(super) fn dense_error_dispatch(instructions: &[Instruction]) -> Option<usize> {
    instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::AddImmediate { d: 0, a: 3 | 30, immediate: -1792 })
    })
}

fn canonicalize_unsigned_transfer_bound(instructions: &mut [Instruction]) {
    if let Some(compare) = instructions.windows(2).position(|window| {
        matches!(window[0], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[1], Instruction::CompareWordImmediate { a: 0, immediate: 2048 })
    }).map(|start| start + 1)
    {
        instructions[compare] = Instruction::CompareLogicalWordImmediate {
            a: 0,
            immediate: 2048,
        };
    }
}

fn schedule_target_access_packet(instructions: &mut [Instruction]) -> bool {
    let Some(call) = target_access_call(instructions) else {
        return false;
    };
    let Some(start) = call.checked_sub(12) else {
        return false;
    };
    let Some(window) = instructions.get(start..=call) else {
        return false;
    };
    if !(
        matches!(window[0], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[1], Instruction::StoreWord { s: 0, a: 1, .. })
            && matches!(window[2], Instruction::AddImmediate { d: 3, a: 1, .. })
            && matches!(window[3], Instruction::LoadWord { d: 4, a: 1, .. })
            && matches!(window[4], Instruction::AddImmediate { d: 5, a: 1, .. })
            && matches!(window[5], Instruction::LoadByteZero { d: 0, a: 1, .. })
            && matches!(window[6], Instruction::AndMaskRecord { a: 0, s: 0, .. })
            && matches!(window[7], Instruction::BranchConditionalForward { .. })
            && matches!(window[8], Instruction::AddImmediate { d: 6, a: 0, immediate: 0 })
            && matches!(window[9], Instruction::Branch { .. })
            && matches!(window[10], Instruction::AddImmediate { d: 6, a: 0, immediate: 1 })
            && matches!(window[11], Instruction::AddImmediate { d: 7, a: 0, immediate: 1 })
            && matches!(window[12], Instruction::BranchAndLink { .. })
    ) {
        return false;
    }
    let length_offset = match instructions[start] {
        Instruction::LoadHalfwordZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    let length_word_offset = match instructions[start + 1] {
        Instruction::StoreWord { offset, .. } => offset,
        _ => unreachable!(),
    };
    let buffer_offset = match instructions[start + 2] {
        Instruction::AddImmediate { immediate, .. } => immediate,
        _ => unreachable!(),
    };
    let address_offset = match instructions[start + 3] {
        Instruction::LoadWord { offset, .. } => offset,
        _ => unreachable!(),
    };
    let options_offset = match instructions[start + 5] {
        Instruction::LoadByteZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    instructions[start] = Instruction::LoadByteZero { d: 0, a: 1, offset: options_offset };
    instructions[start + 1] = Instruction::LoadHalfwordZero { d: 3, a: 1, offset: length_offset };
    instructions[start + 2] = Instruction::AndMaskRecord { a: 0, s: 0, begin: 28, end: 28 };
    instructions[start + 3] = Instruction::StoreWord { s: 3, a: 1, offset: length_word_offset };
    instructions[start + 4] = Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: start + 7 };
    instructions[start + 5] = Instruction::load_immediate(6, 0);
    instructions[start + 6] = Instruction::Branch { target: start + 8 };
    instructions[start + 7] = Instruction::load_immediate(6, 1);
    instructions[start + 8] = Instruction::LoadWord { d: 4, a: 1, offset: address_offset };
    instructions[start + 9] = Instruction::AddImmediate { d: 3, a: 1, immediate: buffer_offset };
    instructions[start + 10] = Instruction::AddImmediate { d: 5, a: 1, immediate: length_word_offset };
    instructions[start + 11] = Instruction::load_immediate(7, 1);
    true
}

pub(super) fn canonicalize_owner_copies(instructions: &mut [Instruction], owner: u8) {
    let copies = instructions.iter().enumerate().filter_map(|(index, instruction)| {
        matches!(instruction, Instruction::Or { a: 3, s, b } if *s == owner && *b == owner)
            .then_some(index)
    }).collect::<Vec<_>>();
    for index in copies {
        let keep_move = matches!(instructions.get(index + 1), Some(Instruction::BranchAndLink { .. }))
            || matches!(
                instructions.get(index + 1..index + 3),
                Some([
                    Instruction::LoadHalfwordZero { d: 4, a: 1, .. },
                    Instruction::BranchAndLink { .. },
                ])
            );
        if !keep_move {
            instructions[index] = Instruction::AddImmediate {
                d: 3,
                a: owner,
                immediate: 0,
            };
        }
    }
}

fn schedule_append_buffer_packet(instructions: &mut [Instruction], owner: u8) -> bool {
    let Some(start) = instructions.windows(5).position(|window| {
        matches!(window[0], Instruction::Or { a: 3, s, b } if s == owner && b == owner)
            && matches!(window[1], Instruction::AddImmediate { d: 4, a: 1, .. })
            && matches!(window[2], Instruction::LoadWord { d: 5, a: 1, .. })
            && matches!(window[3], Instruction::BranchAndLink { .. })
            && matches!(window[4], Instruction::Or { a: 30, s: 3, b: 3 })
    }) else {
        return false;
    };
    let owner_copy = instructions[start].clone();
    let buffer = instructions[start + 1].clone();
    let length = instructions[start + 2].clone();
    instructions[start] = length;
    instructions[start + 1] = owner_copy;
    instructions[start + 2] = buffer;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ArmBody, SwitchArm};

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "transfer".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
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

    fn compact_dispatch() -> Statement {
        Statement::Switch {
            scrutinee: Expression::IntegerLiteral(0),
            arms: [10, 11, 12, 14, 16]
                .into_iter()
                .map(|value| SwitchArm {
                    value,
                    body: ArmBody::Statements(vec![Statement::Break]),
                    falls_through: false,
                })
                .collect(),
            default: Some(ArmBody::Statements(Vec::new())),
        }
    }

    fn conditional(then_body: Vec<Statement>, else_body: Vec<Statement>) -> Statement {
        Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body,
            else_body,
        }
    }

    #[test]
    fn counts_the_hidden_labels_of_the_dispatch_container() {
        let transaction = conditional(
            Vec::new(),
            vec![
                conditional(Vec::new(), Vec::new()),
                conditional(vec![compact_dispatch()], Vec::new()),
            ],
        );
        assert_eq!(
            compact_error_dispatch_container_label_count(&function(vec![transaction])),
            Some(7),
        );
    }

    #[test]
    fn distinguishes_a_top_level_dispatch_from_no_dispatch() {
        assert_eq!(
            compact_error_dispatch_container_label_count(&function(vec![compact_dispatch()])),
            Some(0),
        );
        assert_eq!(
            compact_error_dispatch_container_label_count(&function(vec![conditional(
                Vec::new(),
                Vec::new(),
            )])),
            None,
        );
    }
}
