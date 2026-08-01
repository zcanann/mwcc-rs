//! Compact literal result carried through a call-bearing switch arm.
//!
//! Build 159 keeps the literal initializer solely in its saved GPR, schedules
//! the switch byte load before that initializer, and uses `mr` for the guarded
//! call-result handoff. The generic structured switch remains responsible for
//! the CFG; this module owns only that saved-result transaction.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

impl Generator {
    pub(super) fn schedule_compact_switch_result(&mut self, function: &Function) {
        let Some(local) = compact_switch_result_local(function) else {
            return;
        };
        let Some(home) = self.lookup_general(local) else {
            return;
        };
        let Some(initializer) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction,
                Instruction::AddImmediate { d, a: 0, immediate: 0 } if *d == home)
        }) else {
            return;
        };
        let Some(scrutinee) = self.output.instructions[initializer + 1..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::LoadByteZero { .. }))
            .map(|offset| initializer + 1 + offset)
        else {
            return;
        };
        let Some(result_copy) = self.output.instructions.iter().enumerate().find_map(
            |(index, instruction)| {
                matches!(instruction,
                    Instruction::AddImmediate { d, a: 3, immediate: 0 } if *d == home)
                    .then_some(index)
            },
        ) else {
            return;
        };
        if !matches!(
            self.output.instructions.get(result_copy.wrapping_sub(1)),
            Some(Instruction::BranchAndLink { .. })
        ) {
            return;
        }

        crate::move_instruction_before_retargeting(self, scrutinee, initializer);
        self.output.instructions[result_copy] = Instruction::move_register(home, 3);
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::CompactLiteralHome;
        self.structured_cfg_cleanup_owner = true;
    }
}

fn compact_switch_result_local(function: &Function) -> Option<&str> {
    let Expression::Variable(returned) = function.return_expression.as_ref()? else {
        return None;
    };
    let local = function.locals.iter().find(|local| {
        local.name == *returned
            && matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            && matches!(local.initializer, Some(Expression::IntegerLiteral(0)))
    })?;
    let [Statement::Switch { arms, .. }] = function.statements.as_slice() else {
        return None;
    };
    arms.iter()
        .any(|arm| arm_assigns_call_result(&arm.body, &local.name))
        .then_some(local.name.as_str())
}

fn arm_assigns_call_result(body: &ArmBody, local: &str) -> bool {
    let ArmBody::Statements(statements) = body else {
        return false;
    };
    statements.iter().any(|statement| match statement {
        Statement::Assign {
            name,
            value: Expression::Call { .. },
        } => name == local,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body.iter().chain(else_body).any(|statement| {
            matches!(statement,
                Statement::Assign { name, value: Expression::Call { .. } } if name == local)
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::SwitchArm;

    #[test]
    fn recognizes_a_literal_result_assigned_by_a_switch_arm_call() {
        let function = Function {
            return_type: Type::Int,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "error".into(),
                initializer: Some(Expression::IntegerLiteral(0)),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::Switch {
                scrutinee: Expression::Variable("kind".into()),
                arms: vec![SwitchArm {
                    value: 1,
                    body: ArmBody::Statements(vec![Statement::Assign {
                        name: "error".into(),
                        value: Expression::Call {
                            name: "notify".into(),
                            arguments: Vec::new(),
                        },
                    }]),
                    falls_through: false,
                }],
                default: None,
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("error".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert_eq!(compact_switch_result_local(&function), Some("error"));
    }
}
