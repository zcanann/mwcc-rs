//! Saved-GPR layout for a member-derived receiver inside an object-list loop.
//!
//! The cursor, two incoming call arguments, and the member-derived receiver
//! overlap across calls. Legacy MWCC gives the innermost receiver the highest
//! home, then assigns the cursor and incoming values below it. Keeping this
//! source-level lifetime shape separate avoids teaching the general allocator
//! about one loop's statement order.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) struct StructuredLoopMemberReceiverLayout {
    preferences: [u8; 4],
    cursor: String,
    receiver_member_offset: Option<i16>,
}

impl StructuredLoopMemberReceiverLayout {
    pub(super) fn plan(
        function: &Function,
        eager_locals: &[&LocalDeclaration],
        saved_parameters: &[&Parameter],
        deferred_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        parameter_reuse: &StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        let [cursor] = eager_locals else {
            return None;
        };
        let [receiver] = deferred_locals else {
            return None;
        };
        if saved_parameters.len() != 2
            || home_count != 4
            || !is_pointer(cursor.declared_type)
            || !is_pointer(receiver.declared_type)
        {
            return None;
        }
        let group = deferred.group_if_present(&receiver.name)?;
        if parameter_reuse.home_index(group) != 3 {
            return None;
        }
        let (loop_body, receiver_assignment) =
            function.statements.iter().find_map(|statement| {
                let Statement::Loop { body, .. } = statement else {
                    return None;
                };
                member_receiver_assignment(body, &receiver.name, &cursor.name)
                    .map(|assignment| (body.as_slice(), assignment))
            })?;
        if !loop_body.iter().any(crate::analysis::statement_has_call)
            || !loop_body
                .iter()
                .any(|statement| cursor_advance(statement, &cursor.name))
        {
            return None;
        }

        Some(Self {
            preferences: [30, 29, 28, 31],
            cursor: cursor.name.clone(),
            receiver_member_offset: match receiver_assignment {
                MemberReceiverAssignment::Direct => None,
                MemberReceiverAssignment::Alias { offset } => Some(i16::try_from(offset).ok()?),
            },
        })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.preferences.get(home_index).copied()
    }

    pub(super) fn save_order(&self) -> [usize; 4] {
        [3, 0, 1, 2]
    }

    pub(super) fn frame_slot(&self, home_index: usize) -> usize {
        match home_index {
            0 => 1,
            1 => 2,
            2 => 3,
            3 => 0,
            _ => unreachable!("loop member receiver layout has four homes"),
        }
    }

    /// Materialize `cursor = global->member` with a disposable global-value
    /// lane. MWCC keeps the saved cursor destination free until the final
    /// member load, which matters when that cursor survives every loop call.
    pub(super) fn try_emit_cursor_initializer(
        &self,
        generator: &mut Generator,
        local_name: &str,
        initializer: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if local_name != self.cursor {
            return Ok(false);
        }
        let Expression::Member {
            base,
            offset,
            member_type: Type::Pointer(_) | Type::StructPointer { .. },
            index_stride: None,
        } = initializer
        else {
            return Ok(false);
        };
        let Expression::Variable(global) = base.as_ref() else {
            return Ok(false);
        };
        if generator.locations.contains_key(global)
            || !matches!(
                generator.globals.get(global),
                Some(Type::Pointer(_) | Type::StructPointer { .. })
            )
        {
            return Ok(false);
        }
        let offset = i16::try_from(*offset)
            .map_err(|_| Diagnostic::error("loop cursor member offset is out of range"))?;
        let base = generator.fresh_virtual_general_preferring(5);
        generator.emit_global_load(global, base)?;
        generator.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: base,
            offset,
        });
        Ok(true)
    }

    /// Collapse the source alias pair
    /// `alias = cursor->member; receiver = alias` after branch labels have been
    /// resolved. The semantic recognizer proves the alias has no later use, so
    /// the member can be loaded directly into its callee-saved receiver home.
    pub(super) fn coalesce_receiver_load(
        &self,
        generator: &mut Generator,
        cursor_home: u8,
        receiver_home: u8,
    ) {
        let Some(offset) = self.receiver_member_offset else {
            return;
        };
        let candidates: Vec<_> = generator
            .output
            .instructions
            .windows(2)
            .enumerate()
            .filter_map(|(index, window)| {
                let [Instruction::LoadWord {
                    d: loaded,
                    a,
                    offset: candidate_offset,
                }, move_instruction] = window
                else {
                    return None;
                };
                (*a == cursor_home
                    && *candidate_offset == offset
                    && *loaded != cursor_home
                    && *loaded != receiver_home
                    && *move_instruction == Instruction::move_register(receiver_home, *loaded))
                .then_some((index, *loaded))
            })
            .collect();
        let [(load, _loaded)] = candidates.as_slice() else {
            return;
        };
        let moved = load + 1;
        if generator.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if *target == moved
            )
        }) {
            return;
        }
        let Instruction::LoadWord { d, .. } = &mut generator.output.instructions[*load] else {
            unreachable!("the receiver load was matched")
        };
        *d = receiver_home;
        crate::remove_instruction_retargeting_to_next(generator, moved);
    }
}

fn is_pointer(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

#[derive(Clone, Copy)]
enum MemberReceiverAssignment {
    Direct,
    Alias { offset: u32 },
}

fn member_receiver_assignment(
    body: &[Statement],
    receiver: &str,
    cursor: &str,
) -> Option<MemberReceiverAssignment> {
    let direct = body
        .iter()
        .any(|statement| is_member_assignment(statement, receiver, cursor));
    if direct {
        return Some(MemberReceiverAssignment::Direct);
    }
    body.iter().enumerate().find_map(|(index, statement)| {
        let Statement::Assign {
            name: alias,
            value: Expression::Member { base, offset, .. },
        } = statement
        else {
            return None;
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == cursor) {
            return None;
        }
        let next = body.get(index + 1)?;
        if !matches!(
            next,
            Statement::Assign {
                name,
                value: Expression::Variable(source),
            } if name == receiver && source == alias
        ) || body[index + 2..]
            .iter()
            .any(|statement| statement_reads_name(statement, alias))
        {
            return None;
        }
        Some(MemberReceiverAssignment::Alias { offset: *offset })
    })
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    if matches!(statement, Statement::InlineAsm(_)) {
        return true;
    }
    let mut reads = false;
    super::structured_expression_visit::visit_statement(statement, &mut |expression| {
        reads |= matches!(expression, Expression::Variable(candidate) if candidate == name);
    });
    reads
}

fn is_member_assignment(statement: &Statement, destination: &str, base_name: &str) -> bool {
    matches!(
        statement,
        Statement::Assign {
            name,
            value: Expression::Member { base, .. },
        } if name == destination
            && matches!(base.as_ref(), Expression::Variable(name) if name == base_name)
    )
}

fn cursor_advance(statement: &Statement, cursor: &str) -> bool {
    matches!(
        statement,
        Statement::Assign {
            name,
            value: Expression::Member { base, .. },
        } if name == cursor
            && matches!(base.as_ref(), Expression::Variable(name) if name == cursor)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member_assignment(destination: &str, base: &str, offset: u32) -> Statement {
        Statement::Assign {
            name: destination.into(),
            value: Expression::Member {
                base: Box::new(Expression::Variable(base.into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        }
    }

    #[test]
    fn recognizes_an_adjacent_dead_alias_into_the_saved_receiver() {
        let body = vec![
            member_assignment("temporary", "cursor", 44),
            Statement::Assign {
                name: "receiver".into(),
                value: Expression::Variable("temporary".into()),
            },
            Statement::Expression(Expression::Variable("receiver".into())),
        ];

        assert!(matches!(
            member_receiver_assignment(&body, "receiver", "cursor"),
            Some(MemberReceiverAssignment::Alias { offset: 44 })
        ));
    }

    #[test]
    fn rejects_an_alias_whose_original_value_is_read_later() {
        let body = vec![
            member_assignment("temporary", "cursor", 44),
            Statement::Assign {
                name: "receiver".into(),
                value: Expression::Variable("temporary".into()),
            },
            Statement::Expression(Expression::Variable("temporary".into())),
        ];

        assert!(member_receiver_assignment(&body, "receiver", "cursor").is_none());
    }

    #[test]
    fn recognizes_a_direct_member_receiver_assignment() {
        let body = vec![member_assignment("receiver", "cursor", 44)];

        assert!(matches!(
            member_receiver_assignment(&body, "receiver", "cursor"),
            Some(MemberReceiverAssignment::Direct)
        ));
    }
}
