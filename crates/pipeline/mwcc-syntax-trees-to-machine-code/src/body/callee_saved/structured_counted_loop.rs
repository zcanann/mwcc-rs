//! Canonical CTR ownership for structured signed counted loops.
//!
//! Structured lowering deliberately turns ordinary loops into the same
//! label/goto graph used by the rest of the body emitter. That keeps statement
//! liveness and scheduling correct, but loses the semantic fact that a pure
//! `for (i = 0; i < count; i++)` loop may use PowerPC's count register. This
//! owner records that proof before lowering, then replaces only the resulting
//! entry/tail control-flow instructions after symbolic branches are resolved.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Plan {
    index: String,
    bound: String,
}

impl Plan {
    pub(super) fn recognize(function: &Function) -> Option<Self> {
        let loops: Vec<_> = function
            .statements
            .iter()
            .filter(|statement| matches!(statement, Statement::Loop { .. }))
            .collect();
        let [statement] = loops.as_slice() else {
            return None;
        };
        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        } = statement
        else {
            return None;
        };
        let Expression::Assign {
            target: initializer_target,
            value: initializer_value,
        } = initializer
        else {
            return None;
        };
        let Expression::Variable(index) = initializer_target.as_ref() else {
            return None;
        };
        if !matches!(initializer_value.as_ref(), Expression::IntegerLiteral(0)) {
            return None;
        }
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } = condition
        else {
            return None;
        };
        let (Expression::Variable(condition_index), Expression::Variable(bound)) =
            (left.as_ref(), right.as_ref())
        else {
            return None;
        };
        if condition_index != index
            || !is_unit_step(step, index)
            || index == bound
            || scalar_type(function, index) != Some(Type::Int)
            || scalar_type(function, bound) != Some(Type::Int)
            || body.is_empty()
        {
            return None;
        }
        let address_taken = crate::frame::collect_address_taken(function);
        if address_taken.contains(index)
            || address_taken.contains(bound)
            || body.iter().any(|statement| {
                !is_ctr_safe(statement)
                    || statement_mutates(statement, index)
                    || statement_mutates(statement, bound)
            })
        {
            return None;
        }
        Some(Self {
            index: index.clone(),
            bound: bound.clone(),
        })
    }

    /// Replace the proven label/goto loop's entry and tail with
    /// `mtctr; cmpwi; ble` and `bdnz`. Existing statement emission remains the
    /// sole owner of the body, so local versions and post-loop liveness do not
    /// diverge from non-CTR structured loops.
    pub(super) fn schedule(&self, generator: &mut Generator) -> bool {
        let Some(index) = generator.lookup_general(&self.index) else {
            return false;
        };
        let Some(bound) = generator.lookup_general(&self.bound) else {
            return false;
        };
        let Some(shape) = locate_shape(&generator.output.instructions, index, bound) else {
            return false;
        };
        if generator.output.relocations.iter().any(|relocation| {
            relocation.instruction_index == shape.entry_branch
                || relocation.instruction_index == shape.tail_compare
                || relocation.instruction_index == shape.tail_branch
        }) {
            return false;
        }

        generator.output.instructions[shape.entry_branch] =
            Instruction::MoveToCountRegister { s: bound };
        crate::insert_instruction_retargeting(
            generator,
            shape.entry_branch + 1,
            Instruction::CompareWordImmediate {
                a: bound,
                immediate: 0,
            },
        );
        // One insertion has already moved the old exit by one. The insertion
        // helper then moves the new branch's target once more with everything
        // at or beyond its insertion point.
        crate::insert_instruction_retargeting(
            generator,
            shape.entry_branch + 2,
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: shape.tail_branch + 2,
            },
        );

        let tail_compare = shape.tail_compare + 2;
        let body = match generator.output.instructions[tail_compare + 1] {
            Instruction::BranchConditionalForward { target, .. } => target,
            _ => unreachable!("the located counted-loop tail changed during insertion"),
        };
        generator.output.instructions[tail_compare] =
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: body,
            };
        crate::remove_instruction_retargeting_to_next(generator, tail_compare + 1);
        if self.schedule_preheader_invariants(generator) {
            generator.structured_dense_counted_loop_entry_owner = true;
        }
        true
    }

    fn schedule_preheader_invariants(&self, generator: &mut Generator) -> bool {
        let Some(index) = generator.lookup_general(&self.index) else {
            return false;
        };
        let Some(mut invariants) = locate_preheader_invariants(
            &generator.output.instructions,
            &generator.output.relocations,
            index,
        ) else {
            return false;
        };

        if invariants.upper_base == invariants.upper_value {
            let base = generator.fresh_virtual_general_preferring(3);
            let Instruction::AddImmediateShifted { d, .. } =
                &mut generator.output.instructions[invariants.upper_base_position]
            else {
                return false;
            };
            *d = base;
            let Instruction::AddImmediate { a, .. } =
                &mut generator.output.instructions[invariants.upper_value_position]
            else {
                return false;
            };
            *a = base;
            invariants.upper_base = base;
        }
        let integer_bias = generator.fresh_virtual_general_preferring(23);
        let Instruction::AddImmediateShifted { d, .. } =
            &mut generator.output.instructions[invariants.integer_bias_position]
        else {
            return false;
        };
        *d = integer_bias;
        let Instruction::StoreWord { s, .. } =
            &mut generator.output.instructions[invariants.integer_bias_store]
        else {
            return false;
        };
        *s = integer_bias;
        invariants.integer_bias = integer_bias;

        let Some(Instruction::AddImmediate { a, .. }) = generator
            .output
            .instructions
            .get_mut(invariants.duplicate_upper_value)
        else {
            return false;
        };
        *a = invariants.upper_base;
        crate::remove_instruction_retargeting_to_next(
            generator,
            invariants.duplicate_upper_base,
        );

        let Some(preheader_start) = generator.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate { d, a: 0, immediate: 0 } if *d == index
            )
        }) else {
            return false;
        };
        let ordered = [
            InvariantInstruction::UpperBase(invariants.upper_base),
            InvariantInstruction::StackBase(invariants.stack_base),
            InvariantInstruction::FloatBias(invariants.float_bias),
            InvariantInstruction::UpperValue(invariants.upper_value),
            InvariantInstruction::LowerLimit(invariants.lower_limit),
            InvariantInstruction::IntegerBias(invariants.integer_bias),
        ];
        for (rank, invariant) in ordered.into_iter().enumerate() {
            let Some(from) = generator
                .output
                .instructions
                .iter()
                .position(|instruction| invariant.matches(instruction))
            else {
                return false;
            };
            let to = if rank < 4 {
                preheader_start + rank
            } else {
                preheader_start + rank + 1
            };
            if from <= to {
                return false;
            }
            crate::move_instruction_before_retargeting_source_to_next(generator, from, to);
        }
        // Extending these definitions around the loop changes linear-scan
        // order. Record MWCC's preheader homes as preferences: interference
        // remains authoritative, while the canonical dense loop receives the
        // same volatile/saved split when those lanes are free.
        generator.prefer_virtual_general(invariants.upper_base, 3);
        generator.prefer_virtual_general(invariants.upper_value, 4);
        generator.prefer_virtual_general(index, 6);
        generator.prefer_virtual_general(invariants.lower_limit, 0);
        generator.prefer_virtual_general(invariants.integer_bias, 23);
        generator.prefer_virtual_general(invariants.stack_base, 24);
        true
    }
}

#[derive(Clone, Copy)]
enum InvariantInstruction {
    UpperBase(u8),
    StackBase(u8),
    FloatBias(u8),
    UpperValue(u8),
    LowerLimit(u8),
    IntegerBias(u8),
}

impl InvariantInstruction {
    fn matches(self, instruction: &Instruction) -> bool {
        match self {
            Self::UpperBase(register) => matches!(
                instruction,
                Instruction::AddImmediateShifted { d, a: 0, immediate: 1 }
                    if *d == register
            ),
            Self::StackBase(register) => matches!(
                instruction,
                Instruction::AddImmediate { d, a: 1, .. } if *d == register
            ),
            Self::FloatBias(register) => matches!(
                instruction,
                Instruction::LoadFloatDouble { d, a: 0, offset: 0 } if *d == register
            ),
            Self::UpperValue(register) => matches!(
                instruction,
                Instruction::AddImmediate { d, immediate: -1, .. } if *d == register
            ),
            Self::LowerLimit(register) => matches!(
                instruction,
                Instruction::AddImmediateShifted { d, a: 0, immediate: -1 }
                    if *d == register
            ),
            Self::IntegerBias(register) => matches!(
                instruction,
                Instruction::AddImmediateShifted { d, a: 0, immediate: 17200 }
                    if *d == register
            ),
        }
    }
}

struct PreheaderInvariants {
    upper_base: u8,
    upper_value: u8,
    upper_base_position: usize,
    upper_value_position: usize,
    duplicate_upper_base: usize,
    duplicate_upper_value: usize,
    stack_base: u8,
    float_bias: u8,
    lower_limit: u8,
    integer_bias: u8,
    integer_bias_position: usize,
    integer_bias_store: usize,
}

fn locate_preheader_invariants(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    index: u8,
) -> Option<PreheaderInvariants> {
    let entry = instructions.iter().position(
        |instruction| matches!(instruction, Instruction::MoveToCountRegister { .. }),
    )?;
    let tail = instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { options: 16, target, .. }
                if *target > entry
        )
    })?;
    let body = match instructions[tail] {
        Instruction::BranchConditionalForward { target, .. } => target,
        _ => unreachable!(),
    };
    let mut range = body..tail;

    let upper_pairs: Vec<_> = range
        .clone()
        .filter_map(|position| {
            let Instruction::AddImmediateShifted {
                d: base,
                a: 0,
                immediate: 1,
            } = instructions[position]
            else {
                return None;
            };
            let &Instruction::AddImmediate {
                d: value,
                a,
                immediate: -1,
            } = instructions.get(position + 1)?
            else {
                return None;
            };
            (a == base).then_some((position, base, value))
        })
        .collect();
    let [(upper_position, upper_base, upper_value), (duplicate_position, _, _)] =
        upper_pairs.as_slice()
    else {
        return None;
    };
    let duplicate_upper_value = duplicate_position.checked_add(1)?;

    let stack_base = range.clone().find_map(|position| {
        let Instruction::AddImmediate {
            d,
            a: 1,
            immediate,
        } = instructions[position]
        else {
            return None;
        };
        (immediate >= 0
            && range.clone().any(|consumer| {
                matches!(
                    instructions[consumer],
                    Instruction::LoadFloatDoubleIndexed { a, .. } if a == d
                )
            }))
        .then_some(d)
    })?;
    let (float_bias_position, float_bias) = range.clone().find_map(|position| {
        matches!(
            instructions[position],
            Instruction::LoadFloatDouble { a: 0, offset: 0, .. }
        )
        .then(|| {
            relocations
                .iter()
                .any(|relocation| relocation.instruction_index == position)
                .then(|| match instructions[position] {
                    Instruction::LoadFloatDouble { d, .. } => (position, d),
                    _ => unreachable!(),
                })
        })
        .flatten()
    })?;
    let lower_limit = range.clone().find_map(|position| match instructions[position] {
        Instruction::AddImmediateShifted {
            d,
            a: 0,
            immediate: -1,
        } => Some(d),
        _ => None,
    })?;
    let (integer_bias_position, integer_bias) = range.clone().find_map(|position| {
        match instructions[position] {
            Instruction::AddImmediateShifted {
                d,
                a: 0,
                immediate: 17200,
            } => Some((position, d)),
            _ => None,
        }
    })?;
    let integer_bias_store = (integer_bias_position + 1
        ..integer_bias_position.saturating_add(6).min(tail))
        .find(|position| {
            matches!(instructions[*position], Instruction::StoreWord { s, .. } if s == integer_bias)
        })?;
    if float_bias_position != integer_bias_position + 1
        || integer_bias_store != integer_bias_position + 3
        || !matches!(
            instructions[integer_bias_position + 2],
            Instruction::StoreWord { s, .. } if s != integer_bias
        )
    {
        return None;
    }

    // The counted index must still be initialized directly before CTR. This
    // keeps the preheader rewrite tied to the canonical machine graph rather
    // than a coincidental collection of constants in an unrelated loop.
    if !matches!(
        instructions.get(entry.checked_sub(1)?)?,
        Instruction::AddImmediate { d, a: 0, immediate: 0 } if *d == index
    ) {
        return None;
    }
    Some(PreheaderInvariants {
        upper_base: *upper_base,
        upper_value: *upper_value,
        upper_base_position: *upper_position,
        upper_value_position: upper_position.checked_add(1)?,
        duplicate_upper_base: *duplicate_position,
        duplicate_upper_value,
        stack_base,
        float_bias,
        lower_limit,
        integer_bias,
        integer_bias_position,
        integer_bias_store,
    })
}

fn is_unit_step(expression: &Expression, index: &str) -> bool {
    match expression {
        Expression::PostStep {
            target,
            operator: BinaryOperator::Add,
            pointer_link: None,
        } => matches!(target.as_ref(), Expression::Variable(name) if name == index),
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == index) =>
        {
            matches!(
                value.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if matches!(left.as_ref(), Expression::Variable(name) if name == index)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1))
            )
        }
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct MachineShape {
    entry_branch: usize,
    tail_compare: usize,
    tail_branch: usize,
}

fn locate_shape(instructions: &[Instruction], index: u8, bound: u8) -> Option<MachineShape> {
    for tail_compare in 1..instructions.len().saturating_sub(1) {
        if !matches!(
            instructions[tail_compare],
            Instruction::CompareWord { a, b } if a == index && b == bound
        ) || !matches!(
            instructions[tail_compare - 1],
            Instruction::AddImmediate { d, a, immediate: 1 } if d == index && a == index
        ) {
            continue;
        }
        let Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 0,
            target: body,
        } = instructions[tail_compare + 1]
        else {
            continue;
        };
        let entry_branch = instructions[..body]
            .iter()
            .enumerate()
            .rev()
            .find_map(|(position, instruction)| {
                matches!(instruction, Instruction::Branch { target } if *target == tail_compare)
                    .then_some(position)
            })?;
        if entry_branch == 0
            || !matches!(
                instructions[entry_branch - 1],
                Instruction::AddImmediate { d, a: 0, immediate: 0 } if d == index
            )
        {
            continue;
        }
        return Some(MachineShape {
            entry_branch,
            tail_compare,
            tail_branch: tail_compare + 1,
        });
    }
    None
}

fn scalar_type(function: &Function, name: &str) -> Option<Type> {
    function
        .parameters
        .iter()
        .find_map(|parameter| (parameter.name == name).then_some(parameter.parameter_type))
        .or_else(|| {
            function
                .locals
                .iter()
                .find_map(|local| (local.name == name).then_some(local.declared_type))
        })
}

fn is_ctr_safe(statement: &Statement) -> bool {
    if statement_has_call(statement) {
        return false;
    }
    match statement {
        Statement::Assign { .. } | Statement::Store { .. } | Statement::Expression(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body)
            .all(is_ctr_safe),
        Statement::InlineAsm(_)
        | Statement::Switch { .. }
        | Statement::Loop { .. }
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Return(_) => false,
    }
}

fn statement_mutates(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Expression(value) | Statement::Return(Some(value)) => {
            crate::analysis::expression_assigns_name(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::expression_assigns_name(condition, name)
                || then_body
                    .iter()
                    .chain(else_body)
                    .any(|statement| statement_mutates(statement, name))
        }
        Statement::Switch { .. }
        | Statement::Loop { .. }
        | Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Return(None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};
    use mwcc_syntax_trees::Parameter;

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer: None,
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

    fn function(body: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "counted".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "count".into(),
            }],
            locals: vec![local("i"), local("value")],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("i".into())),
                    value: Box::new(Expression::IntegerLiteral(0)),
                }),
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::Variable("count".into())),
                }),
                step: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("i".into())),
                    value: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable("i".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    }),
                }),
                body,
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
    fn recognizes_a_pure_signed_counted_loop() {
        let plan = Plan::recognize(&function(vec![Statement::Assign {
            name: "value".into(),
            value: Expression::Variable("i".into()),
        }]))
        .expect("the canonical loop should use CTR");
        assert_eq!(plan.index, "i");
        assert_eq!(plan.bound, "count");
    }

    #[test]
    fn rejects_a_call_or_bound_mutation_in_the_body() {
        assert!(Plan::recognize(&function(vec![Statement::Expression(
            Expression::Call {
                name: "consume".into(),
                arguments: Vec::new(),
            },
        )]))
        .is_none());
        assert!(Plan::recognize(&function(vec![Statement::Assign {
            name: "count".into(),
            value: Expression::IntegerLiteral(3),
        }]))
        .is_none());
    }

    #[test]
    fn locates_the_lowered_machine_control_flow() {
        let instructions = vec![
            Instruction::load_immediate(9, 0),
            Instruction::Branch { target: 4 },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 7,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 9,
                immediate: 1,
            },
            Instruction::CompareWord { a: 9, b: 30 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 2,
            },
        ];
        assert_eq!(
            locate_shape(&instructions, 9, 30),
            Some(MachineShape {
                entry_branch: 1,
                tail_compare: 4,
                tail_branch: 5,
            })
        );
    }

    #[test]
    fn locates_a_reusable_dense_loop_preheader() {
        let instructions = vec![
            Instruction::load_immediate(9, 0),
            Instruction::MoveToCountRegister { s: 30 },
            Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 17,
            },
            Instruction::AddImmediateShifted {
                d: 40,
                a: 0,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 41,
                a: 40,
                immediate: -1,
            },
            Instruction::AddImmediateShifted {
                d: 42,
                a: 0,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 43,
                a: 42,
                immediate: -1,
            },
            Instruction::AddImmediateShifted {
                d: 44,
                a: 0,
                immediate: -1,
            },
            Instruction::AddImmediate {
                d: 45,
                a: 1,
                immediate: 8,
            },
            Instruction::LoadFloatDoubleIndexed {
                d: 1,
                a: 45,
                b: 3,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 17200,
            },
            Instruction::LoadFloatDouble {
                d: 2,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 5,
                a: 1,
                offset: 76,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 72,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 9,
                immediate: 1,
            },
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 4,
            },
        ];
        let relocations = vec![Relocation {
            instruction_index: 12,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(0),
        }];
        let invariants = locate_preheader_invariants(&instructions, &relocations, 9)
            .expect("the dense loop invariants should be recognized");
        assert_eq!(invariants.upper_base, 40);
        assert_eq!(invariants.upper_value, 41);
        assert_eq!(invariants.stack_base, 45);
        assert_eq!(invariants.float_bias, 2);
        assert_eq!(invariants.lower_limit, 44);
        assert_eq!(invariants.integer_bias_store, 14);
    }
}
