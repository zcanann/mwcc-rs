//! Compact linkage-first frames for call-filled scalar scratch slots.
//!
//! Build 163 overlaps the logical local-table lane with physical address-taken
//! scalar slots when a retained entry value and returned status are the only
//! other survivors. Keeping that source proof out of generic reconciliation
//! prevents unrelated address-taken locals from being shrunk.

#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredCompactScalarFrame {
    owns_link_register_schedule: bool,
    guarded_call_output_frame: bool,
    packed_switch_frame: bool,
    shared_switch_frame: bool,
}

impl StructuredCompactScalarFrame {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn plan(
        function: &Function,
        switch_source: &Function,
        convention: FrameConvention,
        frame_arrays_empty: bool,
        frame_scalar_locals: &[&LocalDeclaration],
        frame_scalar_parameters: &[&mwcc_syntax_trees::Parameter],
        aggregate_frame_locals: &[&LocalDeclaration],
        eager_saved_locals: &[&LocalDeclaration],
        saved_parameters: &[&mwcc_syntax_trees::Parameter],
        deferred_saved_locals: &[&LocalDeclaration],
    ) -> Option<Self> {
        if convention != FrameConvention::LinkageFirst
            || !frame_arrays_empty
            || !frame_scalar_parameters.is_empty()
            || !aggregate_frame_locals.is_empty()
            || !eager_saved_locals.is_empty()
        {
            return None;
        }
        if eager_saved_locals.is_empty()
            && deferred_saved_locals.is_empty()
            && matches!(saved_parameters, [_])
            && guarded_status_call_chain(function, frame_scalar_locals)
        {
            return Some(Self {
                owns_link_register_schedule: false,
                guarded_call_output_frame: false,
                packed_switch_frame: false,
                shared_switch_frame: false,
            });
        }
        let packed_switch_frame = matches!(saved_parameters, [_])
            && deferred_saved_locals.is_empty()
            && frame_scalar_locals.len() == 5
            && frame_scalar_locals.iter().all(|local| {
                matches!(local.declared_type.width(), 8 | 16 | 32)
            })
            && matches!(function.return_expression.as_ref(), Some(Expression::Variable(name))
                if function.locals.iter().any(|local| local.name == *name
                    && matches!(local.declared_type, Type::Int | Type::UnsignedInt)))
            && switch_count(&switch_source.statements) >= 2;
        let shared_switch_frame = packed_switch_frame
            && frame_scalar_locals
                .iter()
                .filter(|local| local.declared_type.width() == 32)
                .count()
                == 2
            && frame_scalar_locals
                .iter()
                .filter(|local| local.declared_type.width() == 8)
                .count()
                == 3
            && retained_sparse_switch_count(&switch_source.statements) >= 2;
        if packed_switch_frame {
            return Some(Self {
                owns_link_register_schedule: false,
                guarded_call_output_frame: false,
                packed_switch_frame: true,
                shared_switch_frame,
            });
        }
        let ([parameter], [result]) = (saved_parameters, deferred_saved_locals) else {
            return None;
        };
        if !matches!(
            function.return_expression.as_ref(),
            Some(Expression::Variable(name)) if name == &result.name
        ) {
            return None;
        }
        let narrow_dispatch = matches!(frame_scalar_locals,
            [scratch] if matches!(
                scratch.declared_type,
                Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort
            )) && matches!(function.statements.as_slice(), [
            Statement::Assign {
                name,
                value: Expression::IntegerLiteral(_),
            },
            Statement::Expression(Expression::Call { arguments, .. }),
            ..
        ] if name == &result.name
            && matches!(arguments.first(), Some(Expression::Variable(name)) if name == &parameter.name));
        let call_output_frame = frame_scalar_locals.len() >= 2
            && frame_scalar_locals.len() <= 4
            && frame_scalar_locals
                .iter()
                .all(|local| local.declared_type.width() <= 32)
            && matches!(function.statements.first(), Some(Statement::Assign {
                name,
                value: Expression::Call { .. },
            }) if name == &result.name);
        let guarded_call_output_frame = frame_scalar_locals.len() == 4
            && frame_scalar_locals
                .iter()
                .filter(|local| local.declared_type.width() == 32)
                .count()
                == 2
            && frame_scalar_locals
                .iter()
                .filter(|local| local.declared_type.width() == 8)
                .count()
                == 2
            && matches!(function.statements.as_slice(), [
                Statement::If {
                    then_body,
                    else_body,
                    ..
                },
                Statement::Expression(Expression::Call { .. }),
                Statement::Assign {
                    name,
                    value: Expression::Call { .. },
                },
                ..
            ] if else_body.is_empty()
                && then_body.iter().any(|statement| matches!(statement, Statement::Return(_)))
                && name == &result.name)
            && assigned_result_call_count(&function.statements, &result.name) >= 4;
        if !narrow_dispatch && !call_output_frame && !guarded_call_output_frame {
            return None;
        }
        Some(Self {
            owns_link_register_schedule: !guarded_call_output_frame,
            guarded_call_output_frame,
            packed_switch_frame: false,
            shared_switch_frame: false,
        })
    }

    pub(super) fn owns_link_register_schedule(&self) -> bool {
        self.owns_link_register_schedule
    }

    pub(super) fn is_guarded_call_output_frame(&self) -> bool {
        self.guarded_call_output_frame
    }

    pub(super) fn is_shared_switch_frame(&self) -> bool {
        self.shared_switch_frame
    }

    pub(super) fn is_packed_switch_frame(&self) -> bool {
        self.packed_switch_frame
    }
}

fn switch_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => switch_count(then_body) + switch_count(else_body),
            Statement::Loop { body, .. } => switch_count(body),
            Statement::Switch { arms, default, .. } => {
                1 + arms
                    .iter()
                    .map(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => switch_count(body),
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
                    .sum::<usize>()
                    + default.as_ref().map_or(0, |body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => switch_count(body),
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
            }
            _ => 0,
        })
        .sum()
}

fn retained_sparse_switch_count(statements: &[Statement]) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                retained_sparse_switch_count(then_body)
                    + retained_sparse_switch_count(else_body)
            }
            Statement::Loop { body, .. } => retained_sparse_switch_count(body),
            Statement::Switch { arms, default, .. } => {
                usize::from(super::structured_sparse_switch::is_sparse_retained_switch(arms))
                    + arms
                        .iter()
                        .map(|arm| match &arm.body {
                            mwcc_syntax_trees::ArmBody::Statements(body) => {
                                retained_sparse_switch_count(body)
                            }
                            mwcc_syntax_trees::ArmBody::Return(_) => 0,
                        })
                        .sum::<usize>()
                    + default.as_ref().map_or(0, |body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(body) => {
                            retained_sparse_switch_count(body)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    })
            }
            _ => 0,
        })
        .sum()
}

fn assigned_result_call_count(statements: &[Statement], result: &str) -> usize {
    statements
        .iter()
        .map(|statement| match statement {
            Statement::Assign {
                name,
                value: Expression::Call { .. },
            } => usize::from(name == result),
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                assigned_result_call_count(then_body, result)
                    + assigned_result_call_count(else_body, result)
            }
            _ => 0,
        })
        .sum()
}

fn guarded_status_call_chain(
    function: &Function,
    frame_scalar_locals: &[&LocalDeclaration],
) -> bool {
    let [scratch] = frame_scalar_locals else {
        return false;
    };
    if scratch.declared_type.width() != 32 {
        return false;
    }
    let Some(Expression::Variable(result)) = function.return_expression.as_ref() else {
        return false;
    };
    let Some((first, guarded)) = function.statements.split_first() else {
        return false;
    };
    if guarded.len() < 2
        || !matches!(first,
            Statement::Assign {
                name,
                value: Expression::Call { .. },
            } if name == result)
    {
        return false;
    }
    guarded.iter().all(|statement| {
        matches!(statement,
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left,
                    right,
                },
                then_body,
                else_body,
            } if else_body.is_empty()
                && matches!((&**left, &**right),
                    (Expression::Variable(name), Expression::IntegerLiteral(0))
                        if name == result)
                && matches!(then_body.as_slice(), [
                    Statement::Assign {
                        name,
                        value: Expression::Call { .. },
                    }
                ] if name == result))
    })
}

impl Generator {
    /// Complete the measured physical schedule after generic allocation and
    /// latency filling have exposed the compact r30/r31 frame transaction.
    pub(crate) fn finalize_structured_compact_narrow_scalar_frame(&mut self) {
        if !self.structured_compact_narrow_scalar_frame
            || self.output.instructions.len() < 33
            || !matches!(&self.output.instructions[..8], [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
                Instruction::StoreWord { s: 31, a: 1, offset: 20 },
                Instruction::StoreWord { s: 30, a: 1, offset: 16 },
                Instruction::Or { a: 30, s: 3, b: 3 },
                Instruction::AddImmediate { d: 31, a: 0, .. },
                Instruction::AddImmediate { d: 4, a: 0, .. },
            ])
        {
            return;
        }

        let result = self.output.instructions[6].clone();
        let second_argument = self.output.instructions[7].clone();
        self.output.instructions[..8].clone_from_slice(&[
            Instruction::MoveFromLinkRegister { d: 0 },
            second_argument,
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            result,
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 16,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 3,
                immediate: 0,
            },
        ]);

        for instruction in &mut self.output.instructions[8..] {
            if matches!(instruction, Instruction::Or { a: 3, s: 30, b: 30 }) {
                *instruction = Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                };
            }
        }
        if let Some(instruction) = self.output.instructions.iter_mut().find(|instruction| {
            matches!(instruction, Instruction::AddImmediate { d: 31, a: 3, immediate: 0 })
        }) {
            *instruction = Instruction::Or {
                a: 31,
                s: 3,
                b: 3,
            };
        }

        let end = self.output.instructions.len();
        if matches!(&self.output.instructions[end - 7..], [
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::BranchToLinkRegister,
        ]) {
            self.output.instructions[end - 7..].clone_from_slice(&[
                Instruction::Or {
                    a: 3,
                    s: 31,
                    b: 31,
                },
                Instruction::LoadWord {
                    d: 31,
                    a: 1,
                    offset: 20,
                },
                Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset: 16,
                },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 24,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ArmBody, LocalDeclaration, Parameter, SwitchArm};

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
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

    fn function() -> Function {
        Function {
            return_type: Type::Int,
            name: "dispatch".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(Pointee::Int),
                name: "buffer".into(),
            }],
            locals: vec![local("result", Type::Int), local("command", Type::UnsignedChar)],
            statements: vec![
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(1280),
                },
                Statement::Expression(Expression::Call {
                    name: "position".into(),
                    arguments: vec![
                        Expression::Variable("buffer".into()),
                        Expression::IntegerLiteral(0),
                    ],
                }),
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
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
    fn recognizes_the_overlapped_narrow_scratch_layout() {
        let function = function();
        let scratch = [&function.locals[1]];
        let saved = [&function.parameters[0]];
        let deferred = [&function.locals[0]];

        assert!(StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &deferred,
        )
        .is_some());
    }

    #[test]
    fn rejects_a_full_word_scratch_slot() {
        let mut function = function();
        function.locals[1].declared_type = Type::Int;
        let scratch = [&function.locals[1]];
        let saved = [&function.parameters[0]];
        let deferred = [&function.locals[0]];

        assert!(StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &deferred,
        )
        .is_none());
    }

    #[test]
    fn recognizes_a_call_output_scalar_frame() {
        let mut function = function();
        function.locals = vec![
            local("result", Type::Int),
            local("buffer_index", Type::Int),
            local("message", Type::StructPointer { element_size: 32 }),
            local("request_index", Type::Int),
        ];
        function.statements[0] = Statement::Assign {
            name: "result".into(),
            value: Expression::Call {
                name: "acquire".into(),
                arguments: vec![
                    Expression::AddressOf {
                        operand: Box::new(Expression::Variable("buffer_index".into())),
                    },
                    Expression::AddressOf {
                        operand: Box::new(Expression::Variable("message".into())),
                    },
                ],
            },
        };
        let scratch = [&function.locals[1], &function.locals[2], &function.locals[3]];
        let saved = [&function.parameters[0]];
        let deferred = [&function.locals[0]];

        assert!(StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &deferred,
        )
        .is_some());
    }

    #[test]
    fn recognizes_a_guarded_status_chain_with_one_word_scratch() {
        let mut function = function();
        function.locals = vec![local("error", Type::Int), local("word", Type::UnsignedInt)];
        function.return_expression = Some(Expression::Variable("error".into()));
        function.statements = vec![
            Statement::Assign {
                name: "error".into(),
                value: Expression::Call {
                    name: "append".into(),
                    arguments: vec![Expression::Variable("buffer".into())],
                },
            },
            guarded_status_call("read"),
            guarded_status_call("append_word"),
        ];
        let scratch = [&function.locals[1]];
        let saved = [&function.parameters[0]];

        let plan = StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &[],
        )
        .expect("the guarded chain should overlap its scalar table");

        assert!(!plan.owns_link_register_schedule());
    }

    #[test]
    fn recognizes_guarded_mixed_width_scalar_outputs_independent_of_declaration_order() {
        let mut function = function();
        function.locals = vec![
            local("error", Type::Int),
            local("end", Type::UnsignedInt),
            local("start", Type::UnsignedInt),
            local("options", Type::UnsignedChar),
            local("command", Type::UnsignedChar),
        ];
        function.return_expression = Some(Expression::Variable("error".into()));
        function.statements = vec![
            Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: vec![Statement::Return(Some(Expression::Variable("error".into())))],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "position".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "error".into(),
                value: Expression::Call {
                    name: "read_command".into(),
                    arguments: Vec::new(),
                },
            },
            guarded_status_call("read_options"),
            guarded_status_call("read_start"),
            guarded_status_call("read_end"),
        ];
        let scratch = [
            &function.locals[1],
            &function.locals[2],
            &function.locals[3],
            &function.locals[4],
        ];
        let saved = [&function.parameters[0]];
        let deferred = [&function.locals[0]];

        let plan = StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &deferred,
        )
        .expect("the guarded scalar outputs should share their physical frame");

        assert!(plan.is_guarded_call_output_frame());
        assert!(!plan.owns_link_register_schedule());
    }

    #[test]
    fn recognizes_packed_outputs_shared_by_two_sparse_switches() {
        let mut function = function();
        function.locals = vec![
            local("error", Type::Int),
            local("end", Type::UnsignedInt),
            local("start", Type::UnsignedInt),
            local("count", Type::UnsignedChar),
            local("options", Type::UnsignedChar),
            local("command", Type::UnsignedChar),
        ];
        function.return_expression = Some(Expression::Variable("error".into()));
        let first = shared_sparse_switch();
        let second = shared_sparse_switch();
        function.statements = vec![
            first,
            Statement::If {
                condition: Expression::Variable("error".into()),
                then_body: vec![second],
                else_body: Vec::new(),
            },
        ];
        let scratch = [
            &function.locals[1],
            &function.locals[2],
            &function.locals[3],
            &function.locals[4],
            &function.locals[5],
        ];
        let saved = [&function.parameters[0]];

        let plan = StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &[],
        )
        .expect("shared sparse switches should retain a packed scalar frame");

        assert!(plan.is_shared_switch_frame());
        assert!(plan.is_packed_switch_frame());
        assert!(!plan.owns_link_register_schedule());
    }

    #[test]
    fn recognizes_a_mixed_width_multi_switch_frame() {
        let mut function = function();
        function.locals = vec![
            local("error", Type::Int),
            local("length", Type::UnsignedInt),
            local("last", Type::UnsignedShort),
            local("first", Type::UnsignedShort),
            local("options", Type::UnsignedChar),
            local("command", Type::UnsignedChar),
        ];
        function.return_expression = Some(Expression::Variable("error".into()));
        function.statements = vec![
            shared_sparse_switch(),
            Statement::If {
                condition: Expression::Variable("error".into()),
                then_body: vec![shared_sparse_switch()],
                else_body: Vec::new(),
            },
        ];
        let scratch = [
            &function.locals[1],
            &function.locals[2],
            &function.locals[3],
            &function.locals[4],
            &function.locals[5],
        ];
        let saved = [&function.parameters[0]];

        let plan = StructuredCompactScalarFrame::plan(
            &function,
            &function,
            FrameConvention::LinkageFirst,
            true,
            &scratch,
            &[],
            &[],
            &[],
            &saved,
            &[],
        )
        .expect("multiple switches should share the mixed-width scalar frame");

        assert!(plan.is_packed_switch_frame());
        assert!(!plan.is_shared_switch_frame());
    }

    fn shared_sparse_switch() -> Statement {
        Statement::Switch {
            scrutinee: Expression::Variable("options".into()),
            arms: vec![
                SwitchArm {
                    value: 0,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 16,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "count".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
                SwitchArm {
                    value: 1,
                    body: ArmBody::Statements(Vec::new()),
                    falls_through: true,
                },
                SwitchArm {
                    value: 17,
                    body: ArmBody::Statements(vec![Statement::Expression(Expression::Call {
                        name: "range".into(),
                        arguments: Vec::new(),
                    })]),
                    falls_through: false,
                },
            ],
            default: None,
        }
    }

    fn guarded_status_call(name: &str) -> Statement {
        Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(Expression::Variable("error".into())),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Assign {
                name: "error".into(),
                value: Expression::Call {
                    name: name.into(),
                    arguments: Vec::new(),
                },
            }],
            else_body: Vec::new(),
        }
    }
}
