//! Interrupt-protected blocking queue send/receive transactions.
//!
//! The enqueue and dequeue forms share a persistent flag test, an interrupt
//! token, and an owner/payload pair across sleep/wakeup calls. Build 163 gives
//! those roles non-source-order saved homes and hoists the loop-invariant flag
//! mask ahead of the loop, while retaining a separate compare in the retry arm.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Direction {
    Enqueue,
    Dequeue,
}

pub(super) struct StructuredBlockingQueueTransaction {
    owner: String,
    payload: String,
    flags: String,
    interrupt: String,
    direction: Direction,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct StructuredBlockingQueueHomes {
    owner: u8,
    payload: u8,
    flags: u8,
    interrupt: u8,
}

impl StructuredBlockingQueueTransaction {
    pub(super) fn plan(function: &Function) -> Option<Self> {
        if function.return_type != Type::Int
            || function.parameters.len() != 3
            || !matches!(
                function.return_expression,
                Some(Expression::IntegerLiteral(1))
            )
        {
            return None;
        }
        let [owner, payload, flags] = function.parameters.as_slice() else {
            return None;
        };
        let Statement::Assign {
            name: interrupt,
            value: Expression::Call { arguments, .. },
        } = function.statements.first()?
        else {
            return None;
        };
        if !arguments.is_empty() {
            return None;
        }
        let direction = function.statements.iter().find_map(|statement| {
            let Statement::Loop {
                condition: Some(condition),
                ..
            } = statement
            else {
                return None;
            };
            queue_condition(condition, &owner.name)
        })?;
        Some(Self {
            owner: owner.name.clone(),
            payload: payload.name.clone(),
            flags: flags.name.clone(),
            interrupt: interrupt.clone(),
            direction,
        })
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        match self.direction {
            Direction::Enqueue if name == self.owner => Some(28),
            Direction::Enqueue if name == self.payload => Some(29),
            Direction::Enqueue if name == self.interrupt => Some(30),
            Direction::Enqueue if name == self.flags => Some(31),
            Direction::Dequeue if name == self.payload => Some(28),
            Direction::Dequeue if name == self.interrupt => Some(29),
            Direction::Dequeue if name == self.flags => Some(30),
            Direction::Dequeue if name == self.owner => Some(31),
            _ => None,
        }
    }

    pub(super) fn homes(
        &self,
        mut home_for: impl FnMut(&str) -> Option<u8>,
    ) -> Option<StructuredBlockingQueueHomes> {
        Some(StructuredBlockingQueueHomes {
            owner: home_for(&self.owner)?,
            payload: home_for(&self.payload)?,
            flags: home_for(&self.flags)?,
            interrupt: home_for(&self.interrupt)?,
        })
    }

    pub(super) fn save_order(&self, homes: StructuredBlockingQueueHomes) -> [u8; 4] {
        match self.direction {
            Direction::Enqueue => [homes.flags, homes.interrupt, homes.payload, homes.owner],
            Direction::Dequeue => [homes.owner, homes.flags, homes.interrupt, homes.payload],
        }
    }

    pub(super) fn schedule(
        &self,
        generator: &mut Generator,
        homes: StructuredBlockingQueueHomes,
    ) {
        match self.direction {
            Direction::Enqueue => self.schedule_enqueue_body(generator, homes),
            Direction::Dequeue => self.schedule_dequeue_body(generator, homes),
        }
        self.schedule_prologue(
            generator,
            homes.owner,
            homes.payload,
            homes.flags,
            homes.interrupt,
        );
        self.schedule_epilogue(
            generator,
            homes.owner,
            homes.payload,
            homes.flags,
            homes.interrupt,
        );
        let Some((mask, flags)) = generator
            .output
            .instructions
            .iter()
            .enumerate()
            .find_map(|(index, instruction)| {
            matches!(instruction,
                Instruction::AndMaskRecord { begin: 31, end: 31, .. }
            ).then(|| match instruction {
                Instruction::AndMaskRecord { s, .. } => (index, *s),
                _ => unreachable!(),
            })
        }) else {
            return;
        };
        let Some(branch) = mask.checked_sub(1) else {
            return;
        };
        let Instruction::Branch { target } = generator.output.instructions[branch] else {
            return;
        };
        generator.output.instructions[branch] = Instruction::ClearLeftImmediate {
            a: flags,
            s: flags,
            clear: 31,
        };
        generator.output.instructions[mask] = Instruction::Branch { target };
        crate::insert_instruction_retargeting(
            generator,
            mask + 1,
            Instruction::CompareWordImmediate {
                a: flags,
                immediate: 0,
            },
        );
        for instruction in &mut generator.output.instructions {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == mask => *target = mask + 1,
                _ => {}
            }
        }
    }

    fn schedule_enqueue_body(
        &self,
        generator: &mut Generator,
        homes: StructuredBlockingQueueHomes,
    ) {
        let Some(start) = generator
            .output
            .instructions
            .windows(19)
            .position(|window| enqueue_body(window, homes))
        else {
            return;
        };
        let branch = generator.output.instructions[start + 3].clone();
        crate::move_instruction_before_retargeting(generator, start + 18, start + 16);
        generator.output.instructions.splice(
            start..start + 16,
            [
                load_member(6, homes.owner, 20),
                load_member(4, homes.owner, 28),
                Instruction::CompareWord { a: 6, b: 4 },
                branch,
                load_member(0, homes.owner, 24),
                Instruction::AddImmediate {
                    d: 3,
                    a: homes.owner,
                    immediate: 8,
                },
                load_member(5, homes.owner, 16),
                Instruction::Add { d: 4, a: 0, b: 4 },
                Instruction::DivideWord { d: 0, a: 4, b: 6 },
                Instruction::MultiplyLow { d: 0, a: 0, b: 6 },
                Instruction::SubtractFrom { d: 0, a: 0, b: 4 },
                Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: 0,
                    shift: 2,
                },
                Instruction::StoreWordIndexed {
                    s: homes.payload,
                    a: 5,
                    b: 0,
                },
                load_member(4, homes.owner, 28),
                Instruction::AddImmediate {
                    d: 0,
                    a: 4,
                    immediate: 1,
                },
                store_member(0, homes.owner, 28),
            ],
        );
        crate::remove_instruction_retargeting_to_next(generator, start + 18);
        crate::remove_instruction_retargeting_to_next(generator, start + 17);
    }

    fn schedule_dequeue_body(
        &self,
        generator: &mut Generator,
        homes: StructuredBlockingQueueHomes,
    ) {
        let Some(start) = generator
            .output
            .instructions
            .windows(17)
            .position(|window| dequeue_body(window, homes))
        else {
            return;
        };
        generator.output.instructions.splice(
            start..start + 16,
            [
                load_member(0, homes.owner, 24),
                load_member(3, homes.owner, 16),
                Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: 0,
                    shift: 2,
                },
                Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
                Instruction::StoreWord {
                    s: 0,
                    a: homes.payload,
                    offset: 0,
                },
                load_member(5, homes.owner, 24),
                Instruction::move_register(3, homes.owner),
                load_member(4, homes.owner, 20),
                Instruction::AddImmediate {
                    d: 5,
                    a: 5,
                    immediate: 1,
                },
                Instruction::DivideWord { d: 0, a: 5, b: 4 },
                Instruction::MultiplyLow { d: 0, a: 0, b: 4 },
                Instruction::SubtractFrom { d: 0, a: 0, b: 5 },
                store_member(0, homes.owner, 24),
                load_member(4, homes.owner, 28),
                Instruction::AddImmediate {
                    d: 0,
                    a: 4,
                    immediate: -1,
                },
                store_member(0, homes.owner, 28),
            ],
        );
    }

    fn schedule_prologue(
        &self,
        generator: &mut Generator,
        owner: u8,
        payload: u8,
        flags: u8,
        interrupt: u8,
    ) {
        let Some(frame) = generator
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 }))
        else {
            return;
        };
        let Some(call) = generator.output.instructions[frame + 1..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| frame + 1 + offset)
        else {
            return;
        };
        if call != frame + 10 {
            return;
        }
        let scheduled = match self.direction {
            Direction::Enqueue => vec![
                store(flags, 28),
                Instruction::move_register(flags, 5),
                store(interrupt, 24),
                store(payload, 20),
                Instruction::move_register(payload, 4),
                store(owner, 16),
                Instruction::move_register(owner, 3),
            ],
            Direction::Dequeue => vec![
                store(owner, 28),
                Instruction::move_register(owner, 3),
                store(flags, 24),
                Instruction::move_register(flags, 5),
                store(interrupt, 20),
                store(payload, 16),
                Instruction::move_register(payload, 4),
            ],
        };
        generator.output.instructions.splice(frame + 3..call, scheduled);
    }

    fn schedule_epilogue(
        &self,
        generator: &mut Generator,
        owner: u8,
        payload: u8,
        flags: u8,
        interrupt: u8,
    ) {
        let Some(return_index) = generator
            .output
            .instructions
            .iter()
            .rposition(|instruction| matches!(instruction, Instruction::BranchToLinkRegister))
        else {
            return;
        };
        let Some(start) = return_index.checked_sub(7) else {
            return;
        };
        if !matches!(generator.output.instructions[start], Instruction::LoadWord { d: 0, a: 1, offset: 36 }) {
            return;
        }
        let ordered = match self.direction {
            Direction::Enqueue => [flags, interrupt, payload, owner],
            Direction::Dequeue => [owner, flags, interrupt, payload],
        };
        generator.output.instructions.splice(
            start..=return_index,
            [
                load(0, 36),
                load(ordered[0], 28),
                load(ordered[1], 24),
                Instruction::MoveToLinkRegister { s: 0 },
                load(ordered[2], 20),
                load(ordered[3], 16),
                Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
                Instruction::BranchToLinkRegister,
            ],
        );
    }
}

fn store(register: u8, offset: i16) -> Instruction {
    Instruction::StoreWord {
        s: register,
        a: 1,
        offset,
    }
}

fn load(register: u8, offset: i16) -> Instruction {
    Instruction::LoadWord {
        d: register,
        a: 1,
        offset,
    }
}

fn load_member(destination: u8, owner: u8, offset: i16) -> Instruction {
    Instruction::LoadWord {
        d: destination,
        a: owner,
        offset,
    }
}

fn store_member(source: u8, owner: u8, offset: i16) -> Instruction {
    Instruction::StoreWord {
        s: source,
        a: owner,
        offset,
    }
}

fn enqueue_body(instructions: &[Instruction], homes: StructuredBlockingQueueHomes) -> bool {
    matches!(instructions,
        [
            Instruction::LoadWord { a, offset: 20, .. },
            Instruction::LoadWord { a: used_owner, offset: 28, .. },
            Instruction::CompareWord { .. },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { a: first_owner, offset: 24, .. },
            Instruction::LoadWord { a: used_reload_owner, offset: 28, .. },
            Instruction::LoadWord { a: count_reload_owner, offset: 20, .. },
            Instruction::Add { .. },
            Instruction::DivideWord { .. },
            Instruction::MultiplyLow { .. },
            Instruction::SubtractFrom { .. },
            Instruction::LoadWord { a: array_owner, offset: 16, .. },
            Instruction::ShiftLeftImmediate { shift: 2, .. },
            Instruction::StoreWordIndexed { s, .. },
            Instruction::LoadWord { a: final_used_owner, offset: 28, .. },
            Instruction::AddImmediate { immediate: 1, .. },
            Instruction::StoreWord { a: store_owner, offset: 28, .. },
            Instruction::AddImmediate { d: 3, a: wake_owner, immediate: 8 },
            Instruction::BranchAndLink { target },
        ] if *a == homes.owner
            && *used_owner == homes.owner
            && *first_owner == homes.owner
            && *used_reload_owner == homes.owner
            && *count_reload_owner == homes.owner
            && *array_owner == homes.owner
            && *s == homes.payload
            && *final_used_owner == homes.owner
            && *store_owner == homes.owner
            && *wake_owner == homes.owner
            && target == "OSWakeupThread")
}

fn dequeue_body(instructions: &[Instruction], homes: StructuredBlockingQueueHomes) -> bool {
    matches!(instructions,
        [
            Instruction::LoadWord { a, offset: 16, .. },
            Instruction::LoadWord { a: first_owner, offset: 24, .. },
            Instruction::ShiftLeftImmediate { shift: 2, .. },
            Instruction::LoadWordIndexed { .. },
            Instruction::StoreWord { a: output, offset: 0, .. },
            Instruction::LoadWord { a: tail_first_owner, offset: 24, .. },
            Instruction::AddImmediate { immediate: 1, .. },
            Instruction::LoadWord { a: count_owner, offset: 20, .. },
            Instruction::DivideWord { .. },
            Instruction::MultiplyLow { .. },
            Instruction::SubtractFrom { .. },
            Instruction::StoreWord { a: first_store_owner, offset: 24, .. },
            Instruction::LoadWord { a: used_owner, offset: 28, .. },
            Instruction::AddImmediate { immediate: -1, .. },
            Instruction::StoreWord { a: used_store_owner, offset: 28, .. },
            Instruction::Or { a: 3, s: wake_owner, b: wake_owner_copy },
            Instruction::BranchAndLink { target },
        ] if *a == homes.owner
            && *first_owner == homes.owner
            && *output == homes.payload
            && *tail_first_owner == homes.owner
            && *count_owner == homes.owner
            && *first_store_owner == homes.owner
            && *used_owner == homes.owner
            && *used_store_owner == homes.owner
            && *wake_owner == homes.owner
            && *wake_owner_copy == homes.owner
            && target == "OSWakeupThread")
}

fn queue_condition(expression: &Expression, owner: &str) -> Option<Direction> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match operator {
        BinaryOperator::LessEqual
            if member(left, owner, 20) && member(right, owner, 28) =>
        {
            Some(Direction::Enqueue)
        }
        BinaryOperator::Equal
            if member(left, owner, 28)
                && matches!(right.as_ref(), Expression::IntegerLiteral(0)) =>
        {
            Some(Direction::Dequeue)
        }
        _ => None,
    }
}

fn member(expression: &Expression, owner: &str, expected_offset: u32) -> bool {
    matches!(expression,
        Expression::Member { base, offset, .. }
            if *offset == expected_offset
                && matches!(base.as_ref(), Expression::Variable(name) if name == owner))
}
