//! Entry transactions in callers with a retained shared-switch global.
//!
//! In a substantial dispatcher, MWCC plans the early guarded stores together
//! with the later shared-switch lifetime. Two otherwise-independent entry
//! shapes expose that whole-function allocation decision: a pair of distinct
//! scalar-global stores and an error-state publication through the retained
//! pointer. Keep their physical scheduling behind the semantic switch plan so
//! smaller exact functions retain their established store order.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(super) fn schedule_shared_switch_entry_transactions(&mut self, function: &Function) {
        if super::structured_shared_switch_global_value::plan(
            &function.statements,
            &self.globals,
            &self.volatile_globals,
        )
        .is_none()
        {
            return;
        }
        if let Some(pair) = guarded_distinct_global_pair(function) {
            self.schedule_guarded_distinct_global_pair(pair);
        }
        if let Some(publication) = guarded_pointer_publication(function) {
            self.schedule_guarded_pointer_publication(publication);
        }
    }

    fn schedule_guarded_distinct_global_pair(&mut self, pair: GuardedDistinctGlobalPair<'_>) {
        let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: first,
                        a: 0,
                        immediate: first_value,
                    },
                    Instruction::StoreWord {
                        s: first_store,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: second,
                        a: 0,
                        immediate: second_value,
                    },
                    Instruction::StoreWord {
                        s: second_store,
                        a: 0,
                        offset: 0,
                    },
                ] if *first == GENERAL_SCRATCH
                    && *first_store == GENERAL_SCRATCH
                    && *second == GENERAL_SCRATCH
                    && *second_store == GENERAL_SCRATCH
                    && i64::from(*first_value) == pair.first_value
                    && i64::from(*second_value) == pair.second_value
            )
        }) else {
            return;
        };
        if !external_relocation_at(
            &self.output,
            start + 1,
            pair.first_global,
            RelocationKind::EmbSda21,
        ) || !external_relocation_at(
            &self.output,
            start + 3,
            pair.second_global,
            RelocationKind::EmbSda21,
        ) {
            return;
        }

        let first = self.fresh_virtual_general_preferring(3);
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start] else {
            unreachable!("the first constant was recognized")
        };
        *d = first;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!("the first global store was recognized")
        };
        *s = first;
        self.move_instruction_before(start + 2, start + 1);
    }

    fn schedule_guarded_pointer_publication(&mut self, publication: GuardedPointerPublication<'_>) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord {
                        d: member_base,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: constant,
                        a: 0,
                        immediate,
                    },
                    Instruction::StoreWord {
                        s: stored_constant,
                        a: stored_base,
                        offset,
                    },
                    Instruction::LoadWord {
                        d: copied_pointer,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: published,
                        a: anchor,
                        ..
                    },
                    Instruction::StoreWord {
                        s: published_value,
                        a: 0,
                        offset: 0,
                    },
                ] if *constant == GENERAL_SCRATCH
                    && *stored_constant == GENERAL_SCRATCH
                    && *member_base == *stored_base
                    && *offset == publication.member_offset as i16
                    && i64::from(*immediate) == publication.member_value
                    && *copied_pointer != *member_base
                    && *published == GENERAL_SCRATCH
                    && *published_value == GENERAL_SCRATCH
                    && *anchor != 0
            )
        }) else {
            return;
        };
        if !external_relocation_at(
            &self.output,
            start,
            publication.global,
            RelocationKind::EmbSda21,
        ) || !external_relocation_at(
            &self.output,
            start + 3,
            publication.global,
            RelocationKind::EmbSda21,
        ) || !external_relocation_at(
            &self.output,
            start + 5,
            publication.global,
            RelocationKind::EmbSda21,
        ) {
            return;
        }

        let constant = self.fresh_virtual_general_preferring(4);
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!("the publication constant was recognized")
        };
        *d = constant;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 2] else {
            unreachable!("the publication member store was recognized")
        };
        *s = constant;
        self.move_instruction_before(start + 4, start + 2);
    }
}

struct GuardedDistinctGlobalPair<'a> {
    first_global: &'a str,
    first_value: i64,
    second_global: &'a str,
    second_value: i64,
}

fn guarded_distinct_global_pair(function: &Function) -> Option<GuardedDistinctGlobalPair<'_>> {
    function.statements.iter().find_map(|statement| {
        let Statement::If {
            then_body,
            else_body,
            ..
        } = statement
        else {
            return None;
        };
        let [Statement::Store {
            target: Expression::Variable(first_global),
            value: first_value,
        }, Statement::Store {
            target: Expression::Variable(second_global),
            value: second_value,
        }, Statement::Return(None)] = then_body.as_slice()
        else {
            return None;
        };
        let first_value = constant_value(first_value)?;
        let second_value = constant_value(second_value)?;
        (else_body.is_empty() && first_global != second_global && first_value != second_value)
            .then_some(GuardedDistinctGlobalPair {
                first_global,
                first_value,
                second_global,
                second_value,
            })
    })
}

struct GuardedPointerPublication<'a> {
    global: &'a str,
    member_offset: u32,
    member_value: i64,
}

fn guarded_pointer_publication(function: &Function) -> Option<GuardedPointerPublication<'_>> {
    function.statements.iter().find_map(|statement| {
        let Statement::If {
            then_body,
            else_body,
            ..
        } = statement
        else {
            return None;
        };
        let [Statement::Store {
            target:
                Expression::Member {
                    base,
                    offset,
                    index_stride: None,
                    ..
                },
            value: Expression::IntegerLiteral(member_value),
        }, Statement::Assign {
            value: Expression::Variable(copied_global),
            ..
        }, Statement::Store {
            target: Expression::Variable(published_global),
            value: Expression::AddressOf { .. },
        }, ..] = then_body.as_slice()
        else {
            return None;
        };
        let Expression::Variable(member_global) = base.as_ref() else {
            return None;
        };
        (else_body.is_empty()
            && member_global == copied_global
            && member_global == published_global)
            .then_some(GuardedPointerPublication {
                global: member_global,
                member_offset: *offset,
                member_value: *member_value,
            })
    })
}

fn external_relocation_at(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    name: &str,
    kind: RelocationKind,
) -> bool {
    output.relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction_index
            && relocation.kind == kind
            && matches!(
                &relocation.target,
                RelocationTarget::External(target) if target == name
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "dispatch".into(),
            is_static: true,
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

    #[test]
    fn recognizes_a_guarded_pair_of_distinct_global_constants() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("paused".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Variable("pausing".into()),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Store {
                    target: Expression::Variable("executing".into()),
                    value: Expression::Cast {
                        target_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                        operand: Box::new(Expression::IntegerLiteral(0)),
                    },
                },
                Statement::Return(None),
            ],
            else_body: Vec::new(),
        }]);

        let pair = guarded_distinct_global_pair(&function)
            .expect("the guarded stores should form one pair");

        assert_eq!(pair.first_global, "pausing");
        assert_eq!(pair.second_global, "executing");
    }

    #[test]
    fn recognizes_a_member_update_before_pointer_publication() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("fatal".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("executing".into())),
                        offset: 12,
                        member_type: Type::Int,
                        index_stride: None,
                    },
                    value: Expression::IntegerLiteral(-1),
                },
                Statement::Assign {
                    name: "finished".into(),
                    value: Expression::Variable("executing".into()),
                },
                Statement::Store {
                    target: Expression::Variable("executing".into()),
                    value: Expression::AddressOf {
                        operand: Box::new(Expression::Variable("dummy".into())),
                    },
                },
            ],
            else_body: Vec::new(),
        }]);

        let publication = guarded_pointer_publication(&function)
            .expect("the pointer publication should be recognized");

        assert_eq!(publication.global, "executing");
        assert_eq!(publication.member_offset, 12);
        assert_eq!(publication.member_value, -1);
    }
}
