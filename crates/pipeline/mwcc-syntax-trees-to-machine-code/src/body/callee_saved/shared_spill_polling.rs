//! GC/1.1p1 O0 register locals around inlined signed-wide polling calls.
//!
//! Each retained two-local inline scope owns a four-GPR block, in declaration
//! order. Word locals consume descending homes in first-definition order.
//! The parameter's SP+8 image then overlaps the helper's lowest saved GPR.
//! Plan the source scopes first: frame-value lowering would erase that overlap.

use super::*;
use std::collections::{HashMap, HashSet};

enum Operation<'a> {
    Call {
        name: &'a str,
        home: u32,
    },
    Copy {
        source: u32,
        home: u32,
    },
    Wait {
        start_call: &'a str,
        now_call: &'a str,
        start: u32,
        now: u32,
        limit: i16,
    },
    Guard(Vec<Operation<'a>>),
    Repeat {
        body: Vec<Operation<'a>>,
        left: u32,
        right: u32,
        signed: bool,
    },
}

fn word(ty: Type) -> bool {
    matches!(ty, Type::Int | Type::UnsignedInt)
}

fn variable(value: &Expression) -> Option<&str> {
    if let Expression::Variable(name) = value {
        Some(name)
    } else {
        None
    }
}

fn inline_local_ordinal(expanded: &str, callee: &str, source: &str) -> Option<usize> {
    let prefix = format!("__mwcc_inline_{callee}_");
    let (ordinal, name) = expanded.strip_prefix(&prefix)?.split_once('_')?;
    (name == source).then(|| ordinal.parse().ok()).flatten()
}

fn call<'a>(generator: &Generator, value: &'a Expression, ty: Type) -> Option<&'a str> {
    let Expression::Call { name, arguments } = value else {
        return None;
    };
    (arguments.is_empty()
        && generator.call_return_types.get(name) == Some(&ty)
        && !generator.skipped_inline_names.contains(name))
    .then_some(name.as_str())
}

struct Planner<'g, 'a> {
    generator: &'g Generator,
    function: &'a Function,
    homes: HashMap<&'a str, u32>,
    next: u32,
    guards: usize,
    waits: usize,
}

impl<'a> Planner<'_, 'a> {
    fn local_type(&self, name: &str) -> Option<Type> {
        self.function
            .locals
            .iter()
            .find(|local| local.name == name)
            .map(|local| local.declared_type)
    }

    fn word_home(&mut self, name: &'a str) -> Option<u32> {
        if !word(self.local_type(name)?) {
            return None;
        }
        if let Some(&home) = self.homes.get(name) {
            return Some(home);
        }
        self.next = self.next.checked_sub(1)?;
        self.homes.insert(name, self.next);
        Some(self.next)
    }

    /// Verify both generated identities against a retained source declaration.
    /// Do not infer scope boundaries from adjacent expanded locals: successive
    /// inline invocations allocate separate blocks even when lifetimes differ.
    fn pair_scope(&self, start: &str, now: &str) -> Option<bool> {
        for callee in &self.generator.skipped_inline_names {
            let Some(body) = self.generator.inline_bodies.retained_body(callee) else {
                continue;
            };
            let [first, second] = body.locals.as_slice() else {
                continue;
            };
            if !body.parameters.is_empty()
                || first.declared_type != Type::LongLong
                || second.declared_type != Type::LongLong
            {
                continue;
            }
            for (low, high, start_first) in [(start, now, true), (now, start, false)] {
                let Some(ordinal) = inline_local_ordinal(low, callee, &first.name) else {
                    continue;
                };
                if inline_local_ordinal(high, callee, &second.name) == Some(ordinal.checked_add(1)?) {
                    return Some(start_first);
                }
            }
        }
        None
    }

    fn wait(&mut self, statements: &'a [Statement]) -> Option<Operation<'a>> {
        let [Statement::Assign { name: start, value }, Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            step: None,
            condition:
                Some(Expression::Binary {
                    operator: BinaryOperator::LessEqual,
                    left,
                    right,
                }),
            body,
        }, ..] = statements
        else {
            return None;
        };
        let start_call = call(self.generator, value, Type::LongLong)?;
        let [Statement::Assign { name: now, value }] = body.as_slice() else {
            return None;
        };
        let now_call = call(self.generator, value, Type::LongLong)?;
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: current,
            right: initial,
        } = left.as_ref()
        else {
            return None;
        };
        let limit = i16::try_from(constant_value(right)?).ok()?;
        if limit < 0
            || variable(current) != Some(now)
            || variable(initial) != Some(start)
            || start == now
            || self.local_type(start) != Some(Type::LongLong)
            || self.local_type(now) != Some(Type::LongLong)
            || self.homes.contains_key(start.as_str())
            || self.homes.contains_key(now.as_str())
        {
            return None;
        }
        let start_first = self.pair_scope(start, now)?;
        self.next = self.next.checked_sub(4)?;
        let (start_home, now_home) = if start_first {
            (self.next, self.next + 2)
        } else {
            (self.next + 2, self.next)
        };
        self.homes.insert(start, start_home);
        self.homes.insert(now, now_home);
        self.waits += 1;
        Some(Operation::Wait {
            start_call,
            now_call,
            start: start_home,
            now: now_home,
            limit,
        })
    }

    fn block(
        &mut self,
        mut statements: &'a [Statement],
        defined: &mut HashSet<&'a str>,
    ) -> Option<Vec<Operation<'a>>> {
        let mut operations = Vec::new();
        while let Some((statement, rest)) = statements.split_first() {
            if let Some(wait) = self.wait(statements) {
                operations.push(wait);
                statements = &statements[2..];
                continue;
            }
            operations.push(match statement {
                Statement::Assign { name, value } => {
                    let ty = self.local_type(name)?;
                    let operation = if let Some(callee) = call(self.generator, value, ty) {
                        Operation::Call {
                            name: callee,
                            home: self.word_home(name)?.into(),
                        }
                    } else {
                        let source = variable(value)?;
                        if !defined.contains(source) {
                            return None;
                        }
                        Operation::Copy {
                            source: *self.homes.get(source)?,
                            home: self.word_home(name)?.into(),
                        }
                    };
                    defined.insert(name);
                    operation
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } if variable(condition) == Some(self.function.parameters[0].name.as_str())
                    && else_body.is_empty() =>
                {
                    self.guards += 1;
                    Operation::Guard(self.block(then_body, &mut defined.clone())?)
                }
                Statement::Loop {
                    kind: LoopKind::DoWhile,
                    initializer: None,
                    step: None,
                    condition:
                        Some(Expression::Binary {
                            operator: BinaryOperator::NotEqual,
                            left,
                            right,
                        }),
                    body,
                } => {
                    let body = self.block(body, defined)?;
                    let (left, right) = (variable(left)?, variable(right)?);
                    if !defined.contains(left) || !defined.contains(right) {
                        return None;
                    }
                    let (left_type, right_type) = (self.local_type(left)?, self.local_type(right)?);
                    if !word(left_type) || !word(right_type) {
                        return None;
                    }
                    Operation::Repeat {
                        body,
                        left: *self.homes.get(left)?,
                        right: *self.homes.get(right)?,
                        signed: left_type == Type::Int && right_type == Type::Int,
                    }
                }
                _ => return None,
            });
            statements = rest;
        }
        Some(operations)
    }
}

impl Generator {
    pub(crate) fn try_shared_spill_polling(&mut self, function: &Function) -> Compilation<bool> {
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !self.behavior.unoptimized_shared_parameter_spills
            || !word(parameter.parameter_type)
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.asm_body.is_some()
            || !function.inline_asm_blocks.is_empty()
            || function.locals.iter().any(|local| {
                local.initializer.is_some()
                    || local.is_static
                    || local.is_volatile
                    || local.array_length.is_some()
                    || local.attribute_alignment.is_some()
                    || !(word(local.declared_type) || local.declared_type == Type::LongLong)
            })
        {
            return Ok(false);
        }
        let mut planner = Planner {
            generator: self,
            function,
            homes: HashMap::new(),
            next: 32,
            guards: 0,
            waits: 0,
        };
        let Some(operations) = planner.block(&function.statements, &mut HashSet::new()) else {
            return Ok(false);
        };
        if planner.guards != 1
            || !operations
                .iter()
                .any(|operation| matches!(operation, Operation::Guard(_)))
            || planner.waits == 0
            || planner.homes.len() != function.locals.len()
            || planner.next < 14
            || planner.next % 2 != 0
        {
            return Ok(false);
        }
        let first = planner.next;
        // The plan owns only references to source expressions, not generator state.
        let frame_size = 8 + 4 * i16::try_from(32 - first).expect("saved register count");
        self.emit_savegpr_frame_prologue_with_convention(
            first,
            frame_size,
            FrameConvention::LinkageFirst,
        );
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 8,
        });
        self.emit_shared_spill_polling_operations(
            &operations,
            parameter.parameter_type == Type::Int,
        )?;
        self.emit_restgpr_frame_epilogue_with_convention(first, FrameConvention::LinkageFirst);
        Ok(true)
    }

    fn emit_shared_spill_polling_operations(
        &mut self,
        operations: &[Operation<'_>],
        signed_guard: bool,
    ) -> Compilation<()> {
        for operation in operations {
            match operation {
                Operation::Call { name, home } => {
                    self.emit_call(name, &[], None, false)?;
                    self.output
                        .instructions
                        .push(Instruction::move_register(*home, 3));
                }
                Operation::Copy { source, home } => self
                    .output
                    .instructions
                    .push(Instruction::move_register(*home, *source)),
                Operation::Guard(body) => {
                    self.output.instructions.push(Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: 8,
                    });
                    self.output.instructions.push(if signed_guard {
                        Instruction::CompareWordImmediate { a: 0, immediate: 0 }
                    } else {
                        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }
                    });
                    let end = self.fresh_label();
                    self.emit_branch_conditional_to(12, 2, end);
                    self.emit_shared_spill_polling_operations(body, signed_guard)?;
                    self.bind_label(end);
                }
                Operation::Repeat {
                    body,
                    left,
                    right,
                    signed,
                } => {
                    let loop_start = self.fresh_label();
                    self.bind_label(loop_start);
                    self.emit_shared_spill_polling_operations(body, signed_guard)?;
                    self.output.instructions.push(if *signed {
                        Instruction::CompareWord {
                            a: *left,
                            b: *right,
                        }
                    } else {
                        Instruction::CompareLogicalWord {
                            a: *left,
                            b: *right,
                        }
                    });
                    self.emit_branch_conditional_to(4, 2, loop_start);
                }
                Operation::Wait {
                    start_call,
                    now_call,
                    start,
                    now,
                    limit,
                } => {
                    self.emit_call(start_call, &[], None, false)?;
                    self.output.instructions.extend([
                        Instruction::move_register(start + 1, 4),
                        Instruction::move_register(*start, 3),
                    ]);
                    let loop_start = self.fresh_label();
                    self.bind_label(loop_start);
                    self.emit_call(now_call, &[], None, false)?;
                    self.output.instructions.extend([
                        Instruction::move_register(now + 1, 4),
                        Instruction::move_register(*now, 3),
                        Instruction::SubtractFromCarrying {
                            d: 6,
                            a: start + 1,
                            b: now + 1,
                        },
                        Instruction::SubtractFromExtended {
                            d: 3,
                            a: *start,
                            b: *now,
                        },
                        Instruction::load_immediate(5, *limit),
                        Instruction::load_immediate(0, 0),
                        Instruction::XorImmediateShifted {
                            a: 4,
                            s: 0,
                            immediate: 0x8000,
                        },
                        Instruction::XorImmediateShifted {
                            a: 3,
                            s: 3,
                            immediate: 0x8000,
                        },
                        Instruction::SubtractFromCarrying { d: 0, a: 6, b: 5 },
                        Instruction::SubtractFromExtended { d: 3, a: 3, b: 4 },
                        Instruction::SubtractFromExtended { d: 3, a: 4, b: 4 },
                        Instruction::Negate { d: 3, a: 3 },
                        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                    ]);
                    self.emit_branch_conditional_to(12, 2, loop_start);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_identity_uses_the_retained_callee_and_declaration() {
        let name = "__mwcc_inline_wait_2_ticks_17_start_3";
        assert_eq!(
            inline_local_ordinal(name, "wait_2_ticks", "start_3"),
            Some(17)
        );
        assert_eq!(inline_local_ordinal(name, "wait", "start_3"), None);
        assert_eq!(inline_local_ordinal(name, "wait_2_ticks", "start"), None);
        assert_eq!(
            inline_local_ordinal("start_3", "wait_2_ticks", "start_3"),
            None
        );
        assert_eq!(
            inline_local_ordinal("__mwcc_inline_wait_bad_start", "wait", "start"),
            None
        );
    }
}
