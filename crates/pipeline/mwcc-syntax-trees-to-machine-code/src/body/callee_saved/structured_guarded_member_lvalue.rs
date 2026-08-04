//! Retained lvalue addresses for a guarded countdown and callback refresh.
//!
//! Linkage-first MWCC forms the countdown lvalue in `r4` before its positive
//! guard and later forms the same lvalue in the low end of an existing dense
//! saved-register window before an indirect callback.  These are regional
//! lifetimes: turning them into source locals perturbs home grouping for the
//! entire function.  This pass therefore separates source recognition from a
//! verified instruction-stream schedule and only reuses a saved window that
//! the surrounding function already owns.

use super::*;

pub(super) struct Plan {
    object: String,
    member_offset: i16,
    clear_offset: Option<i16>,
}

#[derive(Clone, PartialEq, Eq)]
struct MemberKey {
    object: String,
    offset: i16,
}

pub(super) fn recognize(function: &Function) -> Option<Plan> {
    let mut decrements = Vec::new();
    let mut callbacks = Vec::new();
    let mut clears = Vec::new();
    collect_candidates(
        &function.statements,
        &mut decrements,
        &mut callbacks,
        &mut clears,
    );
    let mut matches = decrements
        .into_iter()
        .filter(|candidate| callbacks.contains(candidate));
    let candidate = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let mut matching_clears = clears
        .into_iter()
        .filter(|clear| clear.object == candidate.object);
    let clear_offset = matching_clears.next().map(|clear| clear.offset);
    if matching_clears.next().is_some() {
        return None;
    }
    Some(Plan {
        object: candidate.object,
        member_offset: candidate.offset,
        clear_offset,
    })
}

impl Plan {
    pub(super) fn schedule(&self, generator: &mut Generator) -> bool {
        let Some(object) = generator
            .locations
            .get(&self.object)
            .map(|location| location.register)
        else {
            return false;
        };
        // The callback address may cross a call only by reusing a window the
        // function already saves.  Never silently expand the ABI footprint.
        if !generator.output.instructions.iter().any(|instruction| {
            matches!(instruction, Instruction::StoreMultipleWord { s, a: 1, .. } if *s <= 27)
        }) {
            return false;
        }
        let Some(decrement) = decrement_region(
            &generator.output.instructions,
            object,
            self.member_offset,
        ) else {
            return false;
        };
        let Some(callback) = callback_region(
            &generator.output.instructions,
            object,
            self.member_offset,
            decrement.store + 1,
        ) else {
            return false;
        };
        let clear = self.clear_offset.and_then(|offset| {
            clear_region(
                &generator.output.instructions,
                object,
                offset,
                callback.store + 1,
            )
        });

        let callback_address = generator.fresh_virtual_general_preferring(27);
        if let Some((clear, clear_member_offset)) = clear.zip(self.clear_offset) {
            let Instruction::StoreByte {
                a,
                offset: store_offset,
                ..
            } =
                &mut generator.output.instructions[clear.store]
            else {
                unreachable!("the guarded byte clear was recognized")
            };
            *a = callback_address;
            *store_offset = 0;
            insert_address(
                generator,
                clear.start,
                callback_address,
                object,
                clear_member_offset,
            );
        }
        let Instruction::StoreWord { a, offset, .. } =
            &mut generator.output.instructions[callback.store]
        else {
            unreachable!("the callback member store was recognized")
        };
        *a = callback_address;
        *offset = 0;
        insert_address(
            generator,
            callback.start,
            callback_address,
            object,
            self.member_offset,
        );

        let Instruction::StoreWord { a, offset, .. } =
            &mut generator.output.instructions[decrement.store]
        else {
            unreachable!("the decrement member store was recognized")
        };
        *a = 4;
        *offset = 0;
        let Instruction::LoadWord { d, .. } =
            &mut generator.output.instructions[decrement.start]
        else {
            unreachable!("the decrement guard load was recognized")
        };
        *d = 3;
        let Instruction::CompareWordImmediate { a, .. } =
            &mut generator.output.instructions[decrement.start + 1]
        else {
            unreachable!("the decrement guard compare was recognized")
        };
        *a = 3;
        insert_address(generator, decrement.start, 4, object, self.member_offset);
        true
    }
}

#[derive(Clone, Copy)]
struct Region {
    start: usize,
    store: usize,
}

fn insert_address(generator: &mut Generator, at: usize, destination: u8, object: u8, offset: i16) {
    crate::insert_instruction_retargeting(
        generator,
        at,
        Instruction::AddImmediate {
            d: destination,
            a: object,
            immediate: offset,
        },
    );
    crate::retarget_instruction_destinations(generator, at + 1, at);
}

fn decrement_region(instructions: &[Instruction], object: u8, offset: i16) -> Option<Region> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: compared, a: load_base, offset: load_offset },
            Instruction::CompareWordImmediate { a: compare, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: value, a: update_base, offset: update_offset },
            Instruction::AddImmediate { d: updated, a: source, immediate: -1 },
            Instruction::StoreWord { s: stored, a: store_base, offset: store_offset },
        ] = window else {
            return None;
        };
        (*load_base == object
            && *load_offset == offset
            && *compare == *compared
            && *update_base == object
            && *update_offset == offset
            && *source == *value
            && *stored == *updated
            && *store_base == object
            && *store_offset == offset)
            .then_some(Region { start, store: start + 5 })
    })
}

fn callback_region(
    instructions: &[Instruction],
    object: u8,
    offset: i16,
    after: usize,
) -> Option<Region> {
    let mut match_found = None;
    for start in after..instructions.len().saturating_sub(2) {
        let [
            Instruction::LoadWord { d: compared, a: load_base, offset: load_offset },
            Instruction::CompareWordImmediate { a: compare, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
        ] = &instructions[start..start + 3] else {
            continue;
        };
        if *load_base != object || *load_offset != offset || *compare != *compared {
            continue;
        }
        let end = (start + 16).min(instructions.len());
        let body = &instructions[start + 3..end];
        let Some(call) = body
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchToLinkRegisterAndLink))
        else {
            continue;
        };
        let Some(store) = body.iter().enumerate().skip(call + 1).find_map(
            |(index, instruction)| {
                matches!(instruction,
                    Instruction::StoreWord { a, offset: store_offset, .. }
                        if *a == object && *store_offset == offset)
                .then_some(start + 3 + index)
            },
        ) else {
            continue;
        };
        if match_found.replace(Region { start, store }).is_some() {
            return None;
        }
    }
    match_found
}

fn clear_region(
    instructions: &[Instruction],
    object: u8,
    offset: i16,
    after: usize,
) -> Option<Region> {
    instructions[after..]
        .windows(7)
        .enumerate()
        .find_map(|(relative, window)| {
            let start = after + relative;
            let [
                Instruction::LoadByteZero { d: compared, a: load_base, offset: load_offset },
                Instruction::CompareLogicalWordImmediate { a: compare, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                _,
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: zero, a: 0, immediate: 0 },
                Instruction::StoreByte { s: stored, a: store_base, offset: store_offset },
            ] = window else {
                return None;
            };
            (*load_base == object
                && *load_offset == offset
                && *compare == *compared
                && *stored == *zero
                && *store_base == object
                && *store_offset == offset)
                .then_some(Region { start, store: start + 6 })
        })
}

fn collect_candidates(
    statements: &[Statement],
    decrements: &mut Vec<MemberKey>,
    callbacks: &mut Vec<MemberKey>,
    clears: &mut Vec<MemberKey>,
) {
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                if else_body.is_empty() {
                    if let Some(candidate) = positive_decrement(condition, then_body) {
                        decrements.push(candidate);
                    }
                    if let Some(candidate) = callback_then_store(condition, then_body) {
                        callbacks.push(candidate);
                    }
                    if let Some(candidate) = direct_call_then_clear(condition, then_body) {
                        clears.push(candidate);
                    }
                }
                collect_candidates(then_body, decrements, callbacks, clears);
                collect_candidates(else_body, decrements, callbacks, clears);
            }
            Statement::Loop { body, .. } => {
                collect_candidates(body, decrements, callbacks, clears)
            }
            _ => {}
        }
    }
}

fn positive_decrement(condition: &Expression, then_body: &[Statement]) -> Option<MemberKey> {
    let Expression::Binary {
        operator: BinaryOperator::Greater,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if crate::analysis::constant_value(right) != Some(0) {
        return None;
    }
    let member = member_key(left)?;
    let matches = match then_body {
        [Statement::Expression(Expression::PostStep {
            target,
            operator: BinaryOperator::Subtract,
            ..
        })] => crate::analysis::structurally_equal(target, left),
        [Statement::Store {
            target,
            value: Expression::IndexedUpdateValue { value },
        }] => matches!(value.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: update_source,
                right: amount,
            } if crate::analysis::structurally_equal(target, left)
                && crate::analysis::structurally_equal(update_source, left)
                && crate::analysis::constant_value(amount) == Some(1)),
        _ => false,
    };
    matches.then_some(member)
}

fn callback_then_store(condition: &Expression, then_body: &[Statement]) -> Option<MemberKey> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if crate::analysis::constant_value(right) != Some(0) {
        return None;
    }
    let member = member_key(left)?;
    let [Statement::Expression(Expression::CallThrough { target, arguments }), Statement::Store { target: stored, .. }] = then_body else {
        return None;
    };
    let callback = member_key(target)?;
    (callback.object == member.object
        && arguments.first().and_then(variable) == Some(member.object.as_str())
        && crate::analysis::structurally_equal(stored, left))
    .then_some(member)
}

fn direct_call_then_clear(condition: &Expression, then_body: &[Statement]) -> Option<MemberKey> {
    let member = member_key(condition)?;
    let [
        Statement::Expression(Expression::Call { arguments, .. }),
        Statement::Store { target, value },
    ] = then_body
    else {
        return None;
    };
    (arguments.first().and_then(variable) == Some(member.object.as_str())
        && crate::analysis::structurally_equal(target, condition)
        && crate::analysis::constant_value(value) == Some(0))
    .then_some(member)
}

fn member_key(expression: &Expression) -> Option<MemberKey> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    Some(MemberKey {
        object: variable(base)?.to_owned(),
        offset: i16::try_from(*offset).ok()?,
    })
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_countdown_callback_pair() {
        let member = || Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 52,
            member_type: Type::UnsignedInt,
            index_stride: None,
        };
        let decrement = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Greater,
                left: Box::new(member()),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Expression(Expression::PostStep {
                target: Box::new(member()),
                operator: BinaryOperator::Subtract,
                pointer_link: None,
            })],
            else_body: Vec::new(),
        };
        let callback = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(member()),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![
                Statement::Expression(Expression::CallThrough {
                    target: Box::new(Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 40,
                        member_type: Type::Pointer(Pointee::Pointer),
                        index_stride: None,
                    }),
                    arguments: vec![Expression::Variable("object".into())],
                }),
                Statement::Store {
                    target: member(),
                    value: Expression::IntegerLiteral(1),
                },
            ],
            else_body: Vec::new(),
        };
        let clear = Statement::If {
            condition: Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset: 3,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
            then_body: vec![
                Statement::Expression(Expression::Call {
                    name: "flush".into(),
                    arguments: vec![Expression::Variable("object".into())],
                }),
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 3,
                        member_type: Type::UnsignedChar,
                        index_stride: None,
                    },
                    value: Expression::IntegerLiteral(0),
                },
            ],
            else_body: Vec::new(),
        };
        let function = Function {
            return_type: Type::Void,
            name: "update".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![decrement, callback, clear],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let plan = recognize(&function).expect("countdown callback pair");
        assert_eq!(plan.object, "object");
        assert_eq!(plan.member_offset, 52);
        assert_eq!(plan.clear_offset, Some(3));
    }
}
