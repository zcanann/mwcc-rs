//! Fixed-count global-array walks mixing calls with an inlined member store.
//!
//! Automatic inline expansion turns a verified leaf setter into a semantic
//! store.  Legacy 2.4.x then strength-reduces every `&array[i]` use to one
//! callee-saved element cursor while retaining the stored constant across the
//! surrounding calls.  Keep that allocation/schedule here instead of teaching
//! either the generic inliner or the call-only loop owner about physical GPRs.

#[allow(unused_imports)]
use super::*;

enum Action<'a> {
    Call(&'a str),
    StoreWord { offset: i16 },
}

struct Plan<'a> {
    array: &'a str,
    actions: Vec<Action<'a>>,
    stored_value: i16,
    stride: i16,
    bound: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn indexed_address<'a>(expression: &'a Expression, index: &str) -> Option<&'a str> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Index {
        base,
        index: selected,
    } = operand.as_ref()
    else {
        return None;
    };
    let Expression::Variable(array) = base.as_ref() else {
        return None;
    };
    variable(selected, index).then_some(array.as_str())
}

fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<Plan<'a>> {
    if !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::Int
        || !matches!(
            function.return_expression,
            Some(Expression::IntegerLiteral(0))
        )
    {
        return None;
    }
    let [counter] = function.locals.as_slice() else {
        return None;
    };
    if counter.declared_type != Type::Int
        || counter.initializer.is_some()
        || counter.array_length.is_some()
        || counter.is_static
        || counter.is_volatile
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer,
        Expression::Assign { target, value }
            if variable(target, &counter.name) && constant_value(value) == Some(0))
        || !matches!(step,
            Expression::Assign { target, value }
                if variable(target, &counter.name)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, &counter.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left, &counter.name) => i16::try_from(constant_value(right)?)
            .ok()
            .filter(|value| *value > 0)?,
        _ => return None,
    };
    if body.len() < 3 {
        return None;
    }

    let mut array = None;
    let mut stored_value = None;
    let mut call_count = 0usize;
    let mut store_count = 0usize;
    let mut actions = Vec::with_capacity(body.len());
    for statement in body {
        let (selected_array, action) = match statement {
            Statement::Expression(Expression::Call { name, arguments }) => {
                let [element] = arguments.as_slice() else {
                    return None;
                };
                call_count += 1;
                (
                    indexed_address(element, &counter.name)?,
                    Action::Call(name.as_str()),
                )
            }
            Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } if matches!(
                member_type,
                Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
            ) => {
                let value = i16::try_from(constant_value(value)?).ok()?;
                if stored_value.is_some_and(|expected| expected != value) {
                    return None;
                }
                stored_value = Some(value);
                store_count += 1;
                (
                    indexed_address(base, &counter.name)?,
                    Action::StoreWord {
                        offset: i16::try_from(*offset).ok()?,
                    },
                )
            }
            _ => return None,
        };
        if array.is_some_and(|expected| expected != selected_array) {
            return None;
        }
        array = Some(selected_array);
        actions.push(action);
    }
    if call_count < 2 || store_count == 0 {
        return None;
    }

    let array = array?;
    let stride = match globals.get(array) {
        Some(Type::Struct { size, .. }) => i16::try_from(*size).ok().filter(|size| *size > 0)?,
        _ => return None,
    };
    if global_array_sizes.get(array).copied()
        != Some(u32::from(bound as u16) * u32::from(stride as u16))
    {
        return None;
    }
    Some(Plan {
        array,
        actions,
        stored_value: stored_value?,
        stride,
        bound,
    })
}

impl Generator {
    pub(super) fn try_indexed_call_store_sequence_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function, &self.globals, &self.global_array_sizes) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.integer_loop_style
                != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        const STORED_VALUE: u8 = 31;
        const ELEMENT: u8 = 30;
        const COUNTER: u8 = 29;
        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![STORED_VALUE, ELEMENT, COUNTER];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
        ]);
        self.emit_address_high(3, plan.array);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: STORED_VALUE,
                a: 1,
                offset: 28,
            },
            Instruction::load_immediate(STORED_VALUE, plan.stored_value),
            Instruction::StoreWord {
                s: ELEMENT,
                a: 1,
                offset: 24,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, plan.array);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: ELEMENT,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: COUNTER,
                a: 1,
                offset: 20,
            },
            Instruction::load_immediate(COUNTER, 0),
        ]);

        let loop_top = self.output.instructions.len();
        for action in plan.actions {
            match action {
                Action::Call(callee) => {
                    self.output
                        .instructions
                        .push(Instruction::move_register(3, ELEMENT));
                    self.record_relocation(RelocationKind::Rel24, callee);
                    self.output.instructions.push(Instruction::BranchAndLink {
                        target: callee.to_owned(),
                    });
                }
                Action::StoreWord { offset } => {
                    self.output.instructions.push(Instruction::StoreWord {
                        s: STORED_VALUE,
                        a: ELEMENT,
                        offset,
                    });
                }
            }
        }
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: COUNTER,
                a: COUNTER,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: ELEMENT,
                a: ELEMENT,
                immediate: plan.stride,
            },
            Instruction::CompareWordImmediate {
                a: COUNTER,
                immediate: plan.bound,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_top,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord {
                d: STORED_VALUE,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: ELEMENT,
                a: 1,
                offset: 24,
            },
            Instruction::LoadWord {
                d: COUNTER,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::LocalDeclaration;

    fn indexed_element(index: &str) -> Expression {
        Expression::AddressOf {
            operand: Box::new(Expression::Index {
                base: Box::new(Expression::Variable("records".into())),
                index: Box::new(Expression::Variable(index.into())),
            }),
        }
    }

    #[test]
    fn recognizes_calls_and_an_inlined_constant_member_store() {
        let function = Function {
            return_type: Type::Int,
            name: "initialize".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "i".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("i".into())),
                    value: Box::new(Expression::IntegerLiteral(0)),
                }),
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(3)),
                }),
                step: Some(Expression::Assign {
                    target: Box::new(Expression::Variable("i".into())),
                    value: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable("i".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    }),
                }),
                body: vec![
                    Statement::Expression(Expression::Call {
                        name: "lock".into(),
                        arguments: vec![indexed_element("i")],
                    }),
                    Statement::Store {
                        target: Expression::Member {
                            base: Box::new(indexed_element("i")),
                            offset: 4,
                            member_type: Type::Int,
                            index_stride: None,
                        },
                        value: Expression::IntegerLiteral(0),
                    },
                    Statement::Expression(Expression::Call {
                        name: "unlock".into(),
                        arguments: vec![indexed_element("i")],
                    }),
                ],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(0)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let globals = std::collections::HashMap::from([(
            "records".into(),
            Type::Struct {
                size: 32,
                align: 4,
            },
        )]);
        let sizes = std::collections::HashMap::from([("records".into(), 96)]);

        let plan = classify(&function, &globals, &sizes).expect("indexed call/store plan");
        assert_eq!(plan.array, "records");
        assert_eq!(plan.stored_value, 0);
        assert_eq!(plan.stride, 32);
        assert_eq!(plan.bound, 3);
        assert_eq!(plan.actions.len(), 3);
    }
}
