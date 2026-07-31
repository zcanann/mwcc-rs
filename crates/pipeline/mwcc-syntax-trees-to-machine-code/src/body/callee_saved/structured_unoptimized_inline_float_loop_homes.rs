//! O0 source-image FPR homes retained through an inlined loop expression.
//!
//! Automatic inline expansion represents a value-returning helper as a comma
//! chain of hygienic assignments. At O0, MWCC keeps those source bindings in a
//! descending saved-FPR window even when ordinary call liveness would permit
//! volatile registers. The returned binding owns the top of the new window;
//! argument bindings follow below it in source order.

use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};

use crate::generator::Generator;

pub(super) struct StructuredUnoptimizedInlineFloatLoopHomes {
    arguments: Vec<String>,
    result: String,
}

impl StructuredUnoptimizedInlineFloatLoopHomes {
    pub(super) fn plan(
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
    ) -> Option<Self> {
        let inline_float_locals: std::collections::HashSet<&str> = ephemeral_locals
            .iter()
            .filter(|local| {
                local.declared_type == Type::Float
                    && local.initializer.is_none()
                    && local.name.starts_with("__mwcc_inline_")
            })
            .map(|local| local.name.as_str())
            .collect();
        if inline_float_locals.len() < 2 {
            return None;
        }

        let mut plans = function.statements.iter().filter_map(|statement| {
            loop_store_assignment_sequence(statement, &inline_float_locals)
        });
        let plan = plans.next()?;
        if plans.next().is_some() {
            return None;
        }
        Some(plan)
    }

    pub(super) fn preference(&self, name: &str, existing_saved_count: u8) -> Option<u8> {
        let top = 31u8.checked_sub(existing_saved_count)?;
        if name == self.result {
            return Some(top);
        }
        self.arguments
            .iter()
            .position(|candidate| candidate == name)
            .and_then(|index| top.checked_sub(u8::try_from(index + 1).ok()?))
    }
}

impl Generator {
    /// Restore the redundant source-binding moves retained by O0 after physical
    /// allocation. These moves extend the already-colored saved-FPR window, so
    /// frame declaration happens here rather than pinning the lanes beforehand.
    pub(crate) fn schedule_unoptimized_inline_float_loop_handoffs(&mut self) {
        if self.behavior.optimization != mwcc_versions::Optimization::O0
            || !self.unoptimized_inline_float_loop_homes
        {
            return;
        }
        let Some(shape) = physical_handoff_shape(&self.output.instructions) else {
            return;
        };
        if !self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -48
                }
            )
        }) || !self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 48
                }
            )
        }) {
            return;
        }

        self.output.instructions[shape.store] = Instruction::StoreFloatSingleIndexed {
            s: shape.stored,
            a: shape.store_base,
            b: shape.store_index,
        };
        crate::insert_instruction_retargeting(
            self,
            shape.round + 1,
            Instruction::FloatMove {
                d: shape.returned,
                b: shape.result,
            },
        );
        crate::insert_instruction_retargeting(
            self,
            shape.round + 2,
            Instruction::FloatMove {
                d: shape.stored,
                b: shape.returned,
            },
        );
        crate::insert_instruction_retargeting(
            self,
            shape.third_input_advance + 1,
            Instruction::FloatMove {
                d: shape.shadow,
                b: shape.first_input,
            },
        );
        rewrite_first_loop_schedule(self);
        let frame_push = self
            .output
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -48
                    }
                )
            })
            .expect("validated inline float frame push disappeared");
        let frame_pop = self
            .output
            .instructions
            .iter()
            .rposition(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 48
                    }
                )
            })
            .expect("validated inline float frame pop disappeared");
        for instruction in &mut self.output.instructions {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class != mwcc_vreg::Class::General {
                    return;
                }
                if *register == 7 {
                    *register = 31;
                } else if *register == 8 {
                    *register = 7;
                }
            });
            match instruction {
                Instruction::LoadFloatSingle { a: 1, offset, .. }
                | Instruction::StoreFloatSingle { a: 1, offset, .. }
                    if (4..=16).contains(offset) =>
                {
                    *offset += 4;
                }
                Instruction::AddImmediate {
                    a: 1, immediate, ..
                } if *immediate == 8 => {
                    *immediate = 12;
                }
                _ => {}
            }
        }
        let first_loop_end = self
            .output
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::CompareWordImmediate {
                        a: 31,
                        immediate: 3
                    }
                )
            })
            .and_then(|compare| compare.checked_add(2))
            .filter(|end| *end <= self.output.instructions.len())
            .expect("validated inline float loop lost its exit branch");
        for instruction in &mut self.output.instructions[first_loop_end..] {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class == mwcc_vreg::Class::General && *register == 3 {
                    *register = 7;
                }
            });
        }
        rewrite_frame_norm_lanes(&mut self.output.instructions[first_loop_end..]);
        rewrite_retained_sqrt_lanes(&mut self.output.instructions[first_loop_end..]);
        if let Instruction::StoreWordWithUpdate { offset, .. } =
            &mut self.output.instructions[frame_push]
        {
            *offset = -32;
        }
        if let Instruction::AddImmediate { immediate, .. } =
            &mut self.output.instructions[frame_pop]
        {
            *immediate = 32;
        }
        crate::insert_instruction_retargeting(
            self,
            frame_push + 1,
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
        );
        crate::insert_instruction_retargeting(
            self,
            frame_pop + 1,
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 28,
            },
        );
        self.frame_size = 32;
        self.callee_saved.push(31);
        self.callee_saved_float = self
            .callee_saved_float
            .max(32u8.saturating_sub(shape.stored));
    }

    /// The generic leaf frame preserves an existing GPR restore before newly
    /// materialized FPR restores. This O0 source-image frame uses the opposite
    /// order: the loop counter is restored after the complete f31..f21 band.
    pub(crate) fn schedule_unoptimized_inline_float_restore_order(&mut self) {
        if !self.unoptimized_inline_float_loop_homes {
            return;
        }
        let Some(restore) = self.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::LoadWord {
                    d: 31,
                    a: 1,
                    offset: 28
                }
            )
        }) else {
            return;
        };
        let Some(last_float_restore) = self.output.instructions.iter().rposition(|instruction| {
            matches!(
                instruction,
                Instruction::LoadFloatDouble { d: 21, a: 1, .. }
            )
        }) else {
            return;
        };
        if restore > last_float_restore {
            return;
        }
        let instruction = self.output.instructions[restore].clone();
        crate::remove_instruction_retargeting_to_next(self, restore);
        let insertion = self
            .output
            .instructions
            .iter()
            .rposition(|candidate| {
                matches!(candidate, Instruction::LoadFloatDouble { d: 21, a: 1, .. })
            })
            .expect("validated f21 restore disappeared")
            + 1;
        crate::insert_instruction_retargeting(self, insertion, instruction);

        let first_float_restore = self
            .output
            .instructions
            .iter()
            .position(|candidate| {
                matches!(
                    candidate,
                    Instruction::PairedSingleQuantizedLoad {
                        d: 31, a: 1, ..
                    }
                )
            })
            .expect("validated f31 restore disappeared");
        let frame_pop = self
            .output
            .instructions
            .iter()
            .rposition(|candidate| {
                matches!(
                    candidate,
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 1..
                    }
                )
            })
            .expect("validated inline float frame pop disappeared");
        for candidate in &mut self.output.instructions[..first_float_restore] {
            if let Instruction::Branch { target } = candidate {
                if *target == frame_pop {
                    *target = first_float_restore;
                }
            }
        }
    }
}

struct PhysicalHandoffShape {
    third_input_advance: usize,
    round: usize,
    store: usize,
    first_input: u8,
    result: u8,
    shadow: u8,
    returned: u8,
    stored: u8,
    store_base: u8,
    store_index: u8,
}

fn physical_handoff_shape(instructions: &[Instruction]) -> Option<PhysicalHandoffShape> {
    let (start, inputs) = instructions
        .windows(6)
        .enumerate()
        .find_map(|(index, window)| {
            let [Instruction::LoadFloatSingle {
                d: first,
                a: first_base,
                offset: 0,
            }, Instruction::AddImmediate {
                d: first_advance,
                a: first_source,
                immediate: 4,
            }, Instruction::LoadFloatSingle {
                d: second,
                a: second_base,
                offset: 0,
            }, Instruction::AddImmediate {
                d: second_advance,
                a: second_source,
                immediate: 4,
            }, Instruction::LoadFloatSingle {
                d: third,
                a: third_base,
                offset: 0,
            }, Instruction::AddImmediate {
                d: third_advance,
                a: third_source,
                immediate: 4,
            }] = window
            else {
                return None;
            };
            (*first_advance == *first_base
                && *first_source == *first_base
                && *second_advance == *second_base
                && *second_source == *second_base
                && *third_advance == *third_base
                && *third_source == *third_base
                && *first == second.saturating_add(1)
                && *second == third.saturating_add(1))
            .then_some((index, [*first, *second, *third]))
        })?;
    let end = instructions[start + 6..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::CompareWordImmediate { .. }))?
        + start
        + 6;
    let (round, result) =
        instructions[start + 6..end]
            .iter()
            .enumerate()
            .find_map(|(relative, instruction)| match instruction {
                Instruction::RoundToSingle { d, b } if d == b => Some((start + 6 + relative, *d)),
                _ => None,
            })?;
    let (store, store_base, store_index) = instructions[round + 1..end]
        .iter()
        .enumerate()
        .find_map(|(relative, instruction)| match instruction {
            Instruction::StoreFloatSingleIndexed { s, a, b } if *s == result => {
                Some((round + 1 + relative, *a, *b))
            }
            _ => None,
        })?;
    let first_input = *inputs.iter().max()?;
    if result != first_input.saturating_add(1) {
        return None;
    }
    let shadow = inputs[2].checked_sub(1)?;
    let returned = shadow.checked_sub(1)?;
    let stored = returned.checked_sub(1)?;
    (stored >= 14).then_some(PhysicalHandoffShape {
        third_input_advance: start + 5,
        round,
        store,
        first_input,
        result,
        shadow,
        returned,
        stored,
        store_base,
        store_index,
    })
}

fn rewrite_first_loop_schedule(generator: &mut Generator) -> bool {
    let Some(start) = generator
        .output
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: 7,
                    shift: 2
                }
            )
        })
    else {
        return false;
    };
    let Some(region) = generator.output.instructions.get(start..start + 37) else {
        return false;
    };
    if !matches!(
        region[1],
        Instruction::LoadFloatSingle {
            d: 26,
            a: 3,
            offset: 0
        }
    ) || !matches!(
        region[3],
        Instruction::LoadFloatSingle {
            d: 25,
            a: 4,
            offset: 0
        }
    ) || !matches!(
        region[5],
        Instruction::LoadFloatSingle {
            d: 24,
            a: 5,
            offset: 0
        }
    ) || !matches!(region[7], Instruction::FloatMove { d: 23, b: 26 })
        || !matches!(region[29], Instruction::RoundToSingle { d: 27, b: 27 })
        || !matches!(region[30], Instruction::FloatMove { d: 22, b: 27 })
        || !matches!(region[31], Instruction::FloatMove { d: 21, b: 22 })
        || !matches!(
            region[33],
            Instruction::StoreFloatSingleIndexed { s: 21, .. }
        )
        || !matches!(
            region[35],
            Instruction::CompareWordImmediate { a: 7, immediate: 3 }
        )
        || !matches!(region[36], Instruction::BranchConditionalForward { .. })
    {
        return false;
    }

    const ORDER: [usize; 37] = [
        5, 6, 3, 4, 1, 2, 7, 14, 15, 16, 8, 9, 10, 11, 12, 13, 18, 19, 20, 25, 26, 27, 17, 21, 22,
        23, 24, 28, 29, 30, 31, 0, 32, 33, 34, 35, 36,
    ];
    let old = region.to_vec();
    for (new_relative, old_relative) in ORDER.iter().copied().enumerate() {
        generator.output.instructions[start + new_relative] = old[old_relative].clone();
    }
    let mut permutation: Vec<_> = (0..generator.output.instructions.len()).collect();
    for (new_relative, old_relative) in ORDER.iter().copied().enumerate() {
        permutation[start + old_relative] = start + new_relative;
    }
    crate::remap_instruction_indices(generator, &permutation);

    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[start + 36]
    else {
        unreachable!("validated first-loop exit branch disappeared");
    };
    *target = start;

    generator.output.instructions[start + 9] = Instruction::LoadFloatDouble {
        d: 5,
        a: 8,
        offset: 0,
    };
    generator.output.instructions[start + 10] =
        Instruction::FloatMultiplySingle { d: 4, a: 1, c: 24 };
    generator.output.instructions[start + 15] =
        Instruction::FloatMultiplyDouble { d: 3, a: 0, c: 26 };
    generator.output.instructions[start + 18] = Instruction::LoadFloatDouble {
        d: 2,
        a: 8,
        offset: 0,
    };
    generator.output.instructions[start + 22] =
        Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 1 };
    generator.output.instructions[start + 23] =
        Instruction::FloatSubtractDouble { d: 0, a: 2, b: 0 };
    generator.output.instructions[start + 24] =
        Instruction::FloatMultiplyDouble { d: 0, a: 0, c: 25 };
    generator.output.instructions[start + 26] = Instruction::FloatAddDouble { d: 0, a: 4, b: 0 };
    generator.output.instructions[start + 27] =
        Instruction::FloatMultiplyDouble { d: 27, a: 5, c: 0 };
    true
}

fn rewrite_frame_norm_lanes(instructions: &mut [Instruction]) -> bool {
    let Some(start) = instructions.windows(11).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 1,
                    offset: 20
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 1,
                    offset: 20
                },
                Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
                Instruction::LoadFloatSingle {
                    d: 2,
                    a: 1,
                    offset: 12
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 1,
                    offset: 12
                },
                Instruction::FloatMultiplySingle { d: 2, a: 2, c: 0 },
                Instruction::LoadFloatSingle {
                    d: 3,
                    a: 1,
                    offset: 16
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 1,
                    offset: 16
                },
                Instruction::FloatMultiplySingle { d: 0, a: 3, c: 0 },
                Instruction::FloatAddSingle { d: 0, a: 2, b: 0 },
                Instruction::FloatAddSingle { d: 30, a: 1, b: 0 },
            ]
        )
    }) else {
        return false;
    };
    instructions[start..start + 11].clone_from_slice(&[
        Instruction::LoadFloatSingle {
            d: 2,
            a: 1,
            offset: 20,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 20,
        },
        Instruction::FloatMultiplySingle { d: 4, a: 2, c: 0 },
        Instruction::LoadFloatSingle {
            d: 2,
            a: 1,
            offset: 12,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 12,
        },
        Instruction::FloatMultiplySingle { d: 3, a: 2, c: 0 },
        Instruction::LoadFloatSingle {
            d: 2,
            a: 1,
            offset: 16,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 1,
            offset: 16,
        },
        Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 },
        Instruction::FloatAddSingle { d: 0, a: 3, b: 0 },
        Instruction::FloatAddSingle { d: 30, a: 4, b: 0 },
    ]);
    true
}

fn rewrite_retained_sqrt_lanes(instructions: &mut [Instruction]) -> bool {
    let Some(start) = instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::FloatReciprocalSqrtEstimate { d: 31, b: 30 }
        )
    }) else {
        return false;
    };
    let Some(end) = instructions[start..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::FloatMove { d: 29, b: 28 }))
        .map(|relative| start + relative)
    else {
        return false;
    };
    for instruction in &mut instructions[start..=end] {
        mwcc_vreg::for_each_register(instruction, |_, class, register| {
            if class != mwcc_vreg::Class::Float {
                return;
            }
            *register = match *register {
                1 => 2,
                2 => 3,
                other => other,
            };
        });
    }
    true
}

fn loop_store_assignment_sequence(
    statement: &Statement,
    inline_float_locals: &std::collections::HashSet<&str>,
) -> Option<StructuredUnoptimizedInlineFloatLoopHomes> {
    let Statement::Loop { body, .. } = statement else {
        return None;
    };
    let [Statement::Store { value, .. }] = body.as_slice() else {
        return None;
    };

    let mut terms = Vec::new();
    flatten_comma(value, &mut terms);
    let Expression::Variable(returned) = terms.pop()? else {
        return None;
    };
    let assignments: Vec<_> = terms
        .into_iter()
        .map(assigned_variable)
        .collect::<Option<_>>()?;
    let (result, arguments) = assignments.split_last()?;
    if result != returned
        || arguments.is_empty()
        || assignments
            .iter()
            .any(|name| !inline_float_locals.contains(name.as_str()))
    {
        return None;
    }

    Some(StructuredUnoptimizedInlineFloatLoopHomes {
        arguments: arguments.to_vec(),
        result: result.clone(),
    })
}

fn flatten_comma<'a>(expression: &'a Expression, output: &mut Vec<&'a Expression>) {
    if let Expression::Comma { left, right } = expression {
        output.push(left);
        flatten_comma(right, output);
    } else {
        output.push(expression);
    }
}

fn assigned_variable(expression: &Expression) -> Option<String> {
    let Expression::Assign { target, .. } = expression else {
        return None;
    };
    let Expression::Variable(name) = target.as_ref() else {
        return None;
    };
    Some(name.clone())
}

#[cfg(test)]
mod tests {
    use super::{physical_handoff_shape, StructuredUnoptimizedInlineFloatLoopHomes};
    use mwcc_machine_code::Instruction;
    use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, LoopKind, Statement, Type};

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Float,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    fn assign(name: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(Expression::Variable(name.into())),
            value: Box::new(value),
        }
    }

    fn comma(left: Expression, right: Expression) -> Expression {
        Expression::Comma {
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn places_the_inline_result_above_its_argument_bindings() {
        let names = [
            "__mwcc_inline_map_0_arg0",
            "__mwcc_inline_map_1_arg1",
            "__mwcc_inline_map_2_result",
        ];
        let locals: Vec<_> = names.iter().map(|name| local(name)).collect();
        let function = Function {
            return_type: Type::Void,
            name: "map".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: locals.clone(),
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![Statement::Store {
                    target: Expression::Variable("output".into()),
                    value: comma(
                        assign(names[0], Expression::FloatLiteral(1.0)),
                        comma(
                            assign(names[1], Expression::FloatLiteral(2.0)),
                            comma(
                                assign(names[2], Expression::FloatLiteral(3.0)),
                                Expression::Variable(names[2].into()),
                            ),
                        ),
                    ),
                }],
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
        };
        let ephemeral: Vec<_> = locals.iter().collect();

        let plan = StructuredUnoptimizedInlineFloatLoopHomes::plan(&function, &ephemeral)
            .expect("inlined comma loop should retain source-image homes");

        assert_eq!(plan.preference(names[2], 4), Some(27));
        assert_eq!(plan.preference(names[0], 4), Some(26));
        assert_eq!(plan.preference(names[1], 4), Some(25));
    }

    #[test]
    fn recognizes_the_colored_three_input_result_handoff() {
        let instructions = vec![
            Instruction::LoadFloatSingle {
                d: 26,
                a: 3,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 4,
            },
            Instruction::LoadFloatSingle {
                d: 25,
                a: 4,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 4,
            },
            Instruction::LoadFloatSingle {
                d: 24,
                a: 5,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 4,
            },
            Instruction::RoundToSingle { d: 27, b: 27 },
            Instruction::StoreFloatSingleIndexed { s: 27, a: 7, b: 0 },
            Instruction::CompareWordImmediate {
                a: 31,
                immediate: 3,
            },
        ];

        let shape = physical_handoff_shape(&instructions)
            .expect("the colored inline loop should retain three handoff lanes");

        assert_eq!((shape.shadow, shape.returned, shape.stored), (23, 22, 21));
        assert_eq!((shape.round, shape.store), (6, 7));
    }
}
