//! Entry-parameter forwarding shared by both arms of a leading call dispatch.
//!
//! A global data-base cache may claim an incoming argument register before the
//! first source statement.  When both arms immediately call the same callee,
//! MWCC first moves surviving entry values directly into their outgoing EABI
//! homes.  The pointer guard's move also defines CR0 and therefore replaces the
//! later zero comparison.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EntryCallForward {
    pub(super) name: String,
    pub(super) incoming: u8,
    pub(super) target: u8,
    pub(super) records_guard: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EntryCallForwarding {
    forwards: Vec<EntryCallForward>,
}

impl EntryCallForwarding {
    pub(super) fn plan(
        function: &Function,
        locations: &std::collections::HashMap<String, Location>,
        call_parameter_types: &std::collections::HashMap<String, Vec<Type>>,
    ) -> Option<Self> {
        if function
            .locals
            .iter()
            .any(|local| local.initializer.as_ref().is_some_and(crate::analysis::expression_has_call))
        {
            return None;
        }
        let [Statement::If {
            condition: Expression::Variable(guard),
            then_body,
            else_body,
        }, ..] = function.statements.as_slice()
        else {
            return None;
        };
        let (then_name, then_arguments) = assigned_call(then_body.first()?)?;
        let (else_name, else_arguments) = assigned_call(else_body.first()?)?;
        if then_name != else_name
            || then_arguments.len() != else_arguments.len()
            || then_arguments.len() > usize::from(Eabi::LAST_GENERAL_ARGUMENT - Eabi::FIRST_GENERAL_ARGUMENT + 1)
        {
            return None;
        }
        let parameter_types = call_parameter_types.get(then_name)?;
        if parameter_types.len() < then_arguments.len()
            || parameter_types[..then_arguments.len()].iter().any(|ty| {
                ty.width() > 32
                    || matches!(ty, Type::Float | Type::Double | Type::Struct { .. })
            })
        {
            return None;
        }
        let guard_parameter = function.parameters.iter().find(|parameter| {
            parameter.name == *guard
                && matches!(
                    parameter.parameter_type,
                    Type::Pointer(_) | Type::StructPointer { .. }
                )
        })?;
        let guard_index = then_arguments
            .iter()
            .zip(else_arguments)
            .position(|(then_argument, else_argument)| {
                variable(then_argument) == Some(guard_parameter.name.as_str())
                    && constant_value(else_argument) == Some(0)
            })?;

        let mut forwards = vec![forward(
            &guard_parameter.name,
            guard_index,
            true,
            locations,
        )?];
        for parameter in &function.parameters {
            if parameter.name == guard_parameter.name {
                continue;
            }
            let Some(index) = then_arguments
                .iter()
                .zip(else_arguments)
                .position(|(then_argument, else_argument)| {
                    variable(then_argument) == Some(parameter.name.as_str())
                        && variable(else_argument) == Some(parameter.name.as_str())
                })
            else {
                continue;
            };
            forwards.push(forward(&parameter.name, index, false, locations)?);
        }
        if forwards.len() < 2 {
            return None;
        }
        let incoming: std::collections::HashSet<_> =
            forwards.iter().map(|forward| forward.incoming).collect();
        let targets: std::collections::HashSet<_> =
            forwards.iter().map(|forward| forward.target).collect();
        if incoming.len() != forwards.len()
            || targets.len() != forwards.len()
            || forwards.iter().any(|forward| {
                forward.incoming == forward.target || incoming.contains(&forward.target)
            })
        {
            return None;
        }
        Some(Self { forwards })
    }

    pub(super) fn emit(&self, generator: &mut Generator) {
        for forward in &self.forwards {
            if forward.records_guard {
                generator
                    .output
                    .instructions
                    .push(Instruction::move_register(forward.target, forward.incoming));
            } else {
                generator.emit_integer_materialization_copy(forward.target, forward.incoming);
            }
            generator
                .locations
                .get_mut(&forward.name)
                .expect("entry forwarding retained its parameter location")
                .register = forward.target;
        }
    }

    pub(super) fn fold_guard_compare(&self, generator: &mut Generator) {
        let Some(guard) = self.forwards.iter().find(|forward| forward.records_guard) else {
            return;
        };
        let Some(copy) = generator.output.instructions.iter().position(|instruction| {
            matches!(instruction,
                Instruction::Or { a, s, b }
                    if *a == guard.target && *s == guard.incoming && *b == guard.incoming)
        }) else {
            return;
        };
        let Some(compare) = generator.output.instructions[copy + 1..]
            .iter()
            .position(|instruction| {
                matches!(instruction,
                    Instruction::CompareWordImmediate { a, immediate: 0 }
                        | Instruction::CompareLogicalWordImmediate { a, immediate: 0 }
                        if *a == guard.target)
            })
            .map(|offset| copy + 1 + offset)
        else {
            return;
        };
        if generator.output.instructions[copy + 1..compare]
            .iter()
            .any(defines_condition_register)
        {
            return;
        }
        generator.output.instructions[copy] = Instruction::OrRecord {
            a: guard.target,
            s: guard.incoming,
            b: guard.incoming,
        };
        crate::remove_instruction_retargeting_to_next(generator, compare);
    }
}

impl Generator {
    /// Fill the linkage slots once allocation has exposed the physical entry
    /// forwarding transaction.  The guard defines CR0 first, the independent
    /// data-anchor high half fills the slot after the LR store, and the shared
    /// scalar reaches its outgoing home immediately before the stack update.
    pub(crate) fn schedule_structured_entry_call_forwarding(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(first_call) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::BranchAndLink { .. })
        }) else {
            return;
        };
        let Some(record) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction,
                    Instruction::OrRecord { a: 5, s: 4, b: 4 })
            })
        else {
            return;
        };
        let has_shared_forward = self.output.instructions[..first_call].iter().any(|instruction| {
            matches!(instruction,
                Instruction::AddImmediate { d: 8, a: 3, immediate: 0 })
        });
        let has_retained_frame = self.output.instructions[..first_call].iter().any(|instruction| {
            matches!(instruction,
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 })
        });
        if !has_shared_forward || !has_retained_frame {
            return;
        }

        if record != 1 {
            crate::move_instruction_before_retargeting(self, record, 1);
        }
        let Some(link_store) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction,
                Instruction::StoreWord { s: 0, a: 1, offset: 4 })
        }) else {
            return;
        };
        let Some(anchor_high) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction,
                    Instruction::AddImmediateShifted { d: 4, a: 0, .. })
            })
        else {
            return;
        };
        let anchor_slot = link_store + 1;
        if anchor_high != anchor_slot {
            crate::move_instruction_before_retargeting(self, anchor_high, anchor_slot);
        }
        let Some(shared) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction,
                    Instruction::AddImmediate { d: 8, a: 3, immediate: 0 })
            })
        else {
            return;
        };
        let shared_slot = anchor_slot + 1;
        if shared != shared_slot {
            crate::move_instruction_before_retargeting(self, shared, shared_slot);
        }

        let mut index = 0;
        while index + 1 < self.output.instructions.len() {
            let (Instruction::AddImmediate { d, a, immediate: first },
                Instruction::AddImmediate { d: next_d, a: next_a, immediate: second }) =
                (&self.output.instructions[index], &self.output.instructions[index + 1])
            else {
                index += 1;
                continue;
            };
            if d != next_d || d != next_a || *a == 0 {
                index += 1;
                continue;
            }
            let Some(combined) = first.checked_add(*second) else {
                index += 1;
                continue;
            };
            self.output.instructions[index] = Instruction::AddImmediate {
                d: *d,
                a: *a,
                immediate: combined,
            };
            crate::remove_instruction_retargeting_to_next(self, index + 1);
        }

        loop {
            let Some(base) = self.output.instructions.windows(3).position(|window| {
                matches!(window,
                    [
                        Instruction::AddImmediate { d: 3, a, immediate: 0 },
                        Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                        Instruction::AddImmediate { d: 4, a: 4, .. },
                    ] if *a >= 14)
            }) else {
                break;
            };
            let enters_base = self.output.instructions.iter().any(|instruction| {
                matches!(instruction,
                    Instruction::Branch { target }
                        | Instruction::BranchConditionalForward { target, .. }
                        if *target == base)
            });
            self.output.instructions[base + 1] = match self.output.instructions[base + 1] {
                Instruction::AddImmediateShifted { immediate, .. } => {
                    Instruction::AddImmediateShifted { d: 3, a: 0, immediate }
                }
                _ => unreachable!("callback high half was matched"),
            };
            self.output.instructions[base + 2] = match self.output.instructions[base + 2] {
                Instruction::AddImmediate { immediate, .. } => {
                    Instruction::AddImmediate { d: 4, a: 3, immediate }
                }
                _ => unreachable!("callback low half was matched"),
            };
            crate::move_instruction_before_retargeting(self, base + 1, base);
            crate::move_instruction_before_retargeting(self, base + 2, base + 1);
            if enters_base {
                for instruction in &mut self.output.instructions {
                    match instruction {
                        Instruction::Branch { target }
                        | Instruction::BranchConditionalForward { target, .. }
                            if *target == base + 2 => *target = base,
                        _ => {}
                    }
                }
            }
        }

        let mut index = 1;
        while index < self.output.instructions.len() {
            if matches!(
                self.output.instructions[index - 1..=index],
                [Instruction::Branch { .. }, Instruction::Branch { .. }]
            ) {
                crate::remove_instruction_retargeting_to_next(self, index);
            } else {
                index += 1;
            }
        }

        if let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(window,
                [
                    Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                    Instruction::AddImmediate { d: 3, a: 3, .. },
                    Instruction::ConditionRegisterClear { .. },
                    Instruction::BranchAndLink { .. },
                ])
        }) {
            crate::move_instruction_before_retargeting(self, start + 2, start + 1);
        }

        if let Some(start) = self.output.instructions.windows(2).position(|window| {
            matches!(window,
                [
                    Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
                    Instruction::AddImmediate { d: 6, a, .. },
                ] if *a >= 14)
        }) {
            crate::move_instruction_before_retargeting(self, start + 1, start);
        }

        while let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(window,
                [
                    Instruction::AddImmediate { d: 4, a, .. },
                    Instruction::AddImmediate { d: 3, a: next_a, .. },
                    Instruction::AddImmediate { d: 5, a: 0, immediate: 3 },
                    Instruction::BranchAndLink { .. },
                ] if a == next_a && *a >= 14)
        }) {
            let enters_start = self.output.instructions.iter().any(|instruction| {
                matches!(instruction,
                    Instruction::Branch { target }
                        | Instruction::BranchConditionalForward { target, .. }
                        if *target == start)
            });
            crate::move_instruction_before_retargeting(self, start + 1, start);
            if enters_start {
                for instruction in &mut self.output.instructions {
                    match instruction {
                        Instruction::Branch { target }
                        | Instruction::BranchConditionalForward { target, .. }
                            if *target == start + 1 => *target = start,
                        _ => {}
                    }
                }
            }
        }

        if let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(window,
                [
                    Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
                    Instruction::StoreWord { s: 0, .. },
                    Instruction::StoreWord { s: 0, .. },
                    Instruction::AddImmediate { d: 3, a: 0, immediate: 1 },
                ])
        }) {
            crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        }

        if let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(window,
                [
                    Instruction::LoadWord { d: 0, a: 1, .. },
                    Instruction::LoadWord { a: 1, .. },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::AddImmediate { d: 1, a: 1, .. },
                    Instruction::BranchToLinkRegister,
                ])
        }) {
            crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        }
    }
}

fn assigned_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Assign {
        value: Expression::Call { name, arguments },
        ..
    } = statement
    else {
        return None;
    };
    Some((name, arguments))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn forward(
    name: &str,
    argument_index: usize,
    records_guard: bool,
    locations: &std::collections::HashMap<String, Location>,
) -> Option<EntryCallForward> {
    Some(EntryCallForward {
        name: name.into(),
        incoming: locations.get(name)?.register,
        target: Eabi::FIRST_GENERAL_ARGUMENT.checked_add(argument_index as u8)?,
        records_guard,
    })
}

fn defines_condition_register(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::OrRecord { .. }
            | Instruction::CompareWordImmediate { .. }
            | Instruction::CompareLogicalWordImmediate { .. }
            | Instruction::CompareWord { .. }
            | Instruction::CompareLogicalWord { .. }
            | Instruction::FloatCompareOrdered { .. }
            | Instruction::FloatCompareUnordered { .. }
            | Instruction::FloatCompareUnorderedField { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn location(register: u8) -> Location {
        Location {
            class: ValueClass::General,
            register,
            signed: false,
            width: 32,
            pointee: None,
            stride: None,
        }
    }

    fn call(callback: &str, third: Expression) -> Statement {
        Statement::Assign {
            name: "result".into(),
            value: Expression::Call {
                name: "spawn".into(),
                arguments: vec![
                    Expression::Variable("thread".into()),
                    Expression::Variable(callback.into()),
                    third,
                    Expression::Variable("stack".into()),
                    Expression::IntegerLiteral(4096),
                    Expression::Variable("priority".into()),
                    Expression::IntegerLiteral(1),
                ],
            },
        }
    }

    #[test]
    fn plans_guard_and_shared_parameter_into_their_final_call_homes() {
        let function = Function {
            return_type: Type::Int,
            name: "wrapper".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter { parameter_type: Type::Int, name: "priority".into() },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::UnsignedChar),
                    name: "pointer".into(),
                },
            ],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "result".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::Variable("pointer".into()),
                then_body: vec![call(
                    "memory_callback",
                    Expression::Variable("pointer".into()),
                )],
                else_body: vec![call(
                    "stream_callback",
                    Expression::IntegerLiteral(0),
                )],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locations = std::collections::HashMap::from([
            ("priority".into(), location(3)),
            ("pointer".into(), location(4)),
        ]);
        let types = std::collections::HashMap::from([(
            "spawn".into(),
            vec![Type::Int; 7],
        )]);

        let plan = EntryCallForwarding::plan(&function, &locations, &types)
            .expect("leading dispatch should forward both parameters");

        assert_eq!(
            plan.forwards,
            vec![
                EntryCallForward {
                    name: "pointer".into(),
                    incoming: 4,
                    target: 5,
                    records_guard: true,
                },
                EntryCallForward {
                    name: "priority".into(),
                    incoming: 3,
                    target: 8,
                    records_guard: false,
                },
            ]
        );
    }
}
