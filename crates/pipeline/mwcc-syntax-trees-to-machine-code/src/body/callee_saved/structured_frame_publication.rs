//! Frame homes for a cursor owner retained only for terminal publication.
//!
//! In a saturated structured loop, MWCC can spill the pointer-to-pointer owner
//! of an address-taken cursor instead of consuming another saved GPR. The
//! cursor and owner then occupy adjacent frame words above the outgoing
//! argument area.

#[allow(unused_imports)]
use super::*;

use super::structured_locals::body_uses_local;
use super::structured_loop_register_pressure::DENSE_SAVED_GPR_COUNT;

pub(super) const OWNER_OFFSET: i16 = 16;
pub(super) const CURSOR_OFFSET: i16 = 20;
pub(super) const LOCAL_REGION_BYTES: i16 = 16;
const OWNER_INCOMING: u8 = Eabi::FIRST_GENERAL_ARGUMENT + 1;

#[derive(Clone)]
pub(super) struct StructuredFramePublication {
    pub(super) parameter: String,
    pub(super) cursor: String,
}

impl StructuredFramePublication {
    pub(super) fn plan(
        function: &Function,
        frame_scalar_locals: &[&LocalDeclaration],
        dense_loop_window: Option<usize>,
    ) -> Option<Self> {
        if dense_loop_window != Some(DENSE_SAVED_GPR_COUNT) {
            return None;
        }
        let [base, owner, first_extent, second_extent] = function.parameters.as_slice() else {
            return None;
        };
        let parameter_types_match = matches!(
            (base.parameter_type, owner.parameter_type),
            (
                Type::Pointer(_) | Type::StructPointer { .. },
                Type::Pointer(Pointee::Pointer)
            )
        ) && matches!(
            (first_extent.parameter_type, second_extent.parameter_type),
            (Type::Int | Type::UnsignedInt, Type::Int | Type::UnsignedInt)
        );
        if !parameter_types_match {
            return None;
        }
        let [cursor] = frame_scalar_locals else {
            return None;
        };
        let cursor_matches = matches!(
            cursor.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        ) && matches!(
            cursor.initializer.as_ref(),
            Some(Expression::Dereference { pointer })
                if matches!(pointer.as_ref(), Expression::Variable(name) if name == &owner.name)
        );
        if !cursor_matches {
            return None;
        }
        let (terminal, prefix) = function.statements.split_last()?;
        let terminal_matches = matches!(
            terminal,
            Statement::Store {
                target,
                value: Expression::Variable(value),
            } if value == &cursor.name
                && matches!(
                    target,
                    Expression::Dereference { pointer }
                        if matches!(
                            pointer.as_ref(),
                            Expression::Variable(name) if name == &owner.name
                        )
                )
        );
        let prefix_reads_owner = body_uses_local(prefix, &owner.name);
        if !terminal_matches || prefix_reads_owner {
            return None;
        }
        Some(Self {
            parameter: owner.name.clone(),
            cursor: cursor.name.clone(),
        })
    }

    /// The saturated frame's three retained parameters are discovered in
    /// reverse source order. MWCC assigns the two extents from r14 upward and
    /// keeps the primary aggregate base in r26.
    pub(super) fn saved_parameter_preference(&self, home_index: usize) -> Option<u8> {
        [14, 15, 26].get(home_index).copied()
    }
}

impl Generator {
    /// Keep the just-loaded cursor-owner alias through the first packet. The
    /// post-increment still publishes the advanced cursor to its frame slot,
    /// while the initial reload and packet-base scratch become unnecessary.
    pub(crate) fn schedule_structured_frame_publication_entry(&mut self) {
        let has_owner_slot = self.frame_slots.values().any(|slot| {
            slot.offset == OWNER_OFFSET
                && slot.parameter_register.is_none()
                && slot.value_type == Type::Pointer(Pointee::Pointer)
        });
        if !has_owner_slot {
            return;
        }
        let Some(start) = publication_entry_packet(&self.output.instructions) else {
            return;
        };
        let (cursor, advanced, command, zero, reload) = match &self.output.instructions
            [start..start + 8]
        {
            [Instruction::LoadWord { d: cursor, .. }, Instruction::AddImmediate { d: advanced, .. }, Instruction::StoreWord { .. }, Instruction::AddImmediateShifted {
                immediate: command, ..
            }, Instruction::StoreWord { .. }, Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            }, Instruction::StoreWord { .. }, reload] => {
                (*cursor, *advanced, *command, *zero, reload.clone())
            }
            _ => unreachable!("publication entry packet was matched"),
        };
        let replacement = [
            Instruction::AddImmediate {
                d: advanced,
                a: OWNER_INCOMING,
                immediate: 8,
            },
            Instruction::load_immediate_shifted(cursor, command),
            Instruction::StoreWord {
                s: advanced,
                a: 1,
                offset: CURSOR_OFFSET,
            },
            Instruction::load_immediate(zero, 0),
            Instruction::StoreWord {
                s: cursor,
                a: OWNER_INCOMING,
                offset: 0,
            },
            Instruction::StoreWord {
                s: zero,
                a: OWNER_INCOMING,
                offset: 4,
            },
            reload,
        ];
        self.output
            .instructions
            .splice(start..start + 8, replacement);
        let old_len = self.output.instructions.len() + 1;
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != start);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < start {
                    old
                } else if old == start {
                    start
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn publication_entry_packet(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: cursor,
                    a: 1,
                    offset: CURSOR_OFFSET,
                },
                Instruction::AddImmediate {
                    d: advanced,
                    a: add_base,
                    immediate: 8,
                },
                Instruction::StoreWord {
                    s: stored_advance,
                    a: 1,
                    offset: CURSOR_OFFSET,
                },
                Instruction::AddImmediateShifted {
                    d: command,
                    a: 0,
                    ..
                },
                Instruction::StoreWord {
                    s: stored_command,
                    a: packet,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: zero,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: stored_zero,
                    a: zero_packet,
                    offset: 4,
                },
                Instruction::LoadWord {
                    d: reload,
                    a: 1,
                    offset: CURSOR_OFFSET,
                },
            ] if cursor == add_base
                && cursor == packet
                && cursor == zero_packet
                && cursor == reload
                && advanced == stored_advance
                && command == stored_command
                && zero == stored_zero
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    fn parameter(parameter_type: Type, name: &str) -> Parameter {
        Parameter {
            parameter_type,
            name: name.into(),
        }
    }

    fn cursor() -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 8 },
            name: "cursor".into(),
            initializer: Some(Expression::Dereference {
                pointer: Box::new(Expression::Variable("owner".into())),
            }),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Void,
            name: "publish".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                parameter(Type::StructPointer { element_size: 32 }, "base"),
                parameter(Type::Pointer(Pointee::Pointer), "owner"),
                parameter(Type::Int, "width"),
                parameter(Type::Int, "height"),
            ],
            locals: vec![cursor()],
            statements: vec![Statement::Store {
                target: Expression::Dereference {
                    pointer: Box::new(Expression::Variable("owner".into())),
                },
                value: Expression::Variable("cursor".into()),
            }],
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

    #[test]
    fn recognizes_terminal_cursor_publication_under_dense_pressure() {
        let function = function();
        assert!(StructuredFramePublication::plan(
            &function,
            &[&function.locals[0]],
            Some(DENSE_SAVED_GPR_COUNT),
        )
        .is_some());
    }

    #[test]
    fn rejects_an_owner_read_before_terminal_publication() {
        let mut function = function();
        function.statements.insert(
            0,
            Statement::Expression(Expression::Variable("owner".into())),
        );
        assert!(StructuredFramePublication::plan(
            &function,
            &[&function.locals[0]],
            Some(DENSE_SAVED_GPR_COUNT),
        )
        .is_none());
    }

    #[test]
    fn recognizes_the_initial_cursor_packet() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: CURSOR_OFFSET,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: CURSOR_OFFSET,
            },
            Instruction::load_immediate_shifted(0, -6400),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: CURSOR_OFFSET,
            },
        ];
        assert_eq!(publication_entry_packet(&instructions), Some(0));
    }
}
