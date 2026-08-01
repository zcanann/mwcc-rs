//! Compact build-163 frame for a context forwarded after a setup call.
//!
//! A wrapper shaped as `setup(); consume(second_parameter, literal)` keeps only
//! the second parameter across the setup call. MWCC gives that value one saved
//! GPR but does not retain the otherwise-unused incoming parameter table.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_linkage_first_forwarded_context_frame(
        &mut self,
        function: &Function,
    ) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !forwarded_context_wrapper(function)
            || self.frame_size != 24
            || self.output.instructions.len() < 10
        {
            return;
        }

        let saved = match self.output.instructions.get(..5) {
            Some([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
                Instruction::StoreWord { s, a: 1, offset: 20 },
                Instruction::Or { a, s: 4, b: 4 },
            ]) if s == a && (14..=31).contains(s) => *s,
            _ => return,
        };
        let end = self.output.instructions.len();
        if !matches!(
            &self.output.instructions[end - 5..],
            [
                Instruction::LoadWord { d: 0, a: 1, offset: 28 },
                Instruction::LoadWord { d, a: 1, offset: 20 },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
                Instruction::BranchToLinkRegister,
            ] if *d == saved
        ) {
            return;
        }

        self.output.instructions[2] = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        };
        self.output.instructions[3] = Instruction::StoreWord {
            s: saved,
            a: 1,
            offset: 12,
        };
        self.output.instructions.splice(
            end - 5..end,
            [
                Instruction::LoadWord {
                    d: saved,
                    a: 1,
                    offset: 12,
                },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 16,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ],
        );
        self.frame_size = 16;
    }
}

fn forwarded_context_wrapper(function: &Function) -> bool {
    if function.return_type != Type::Void
        || function.parameters.len() != 2
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return false;
    }
    let [
        Statement::Expression(Expression::Call {
            arguments: setup_arguments,
            ..
        }),
        Statement::Expression(Expression::Call {
            arguments: forwarded_arguments,
            ..
        }),
    ] = function.statements.as_slice()
    else {
        return false;
    };
    matches!(
        (setup_arguments.as_slice(), forwarded_arguments.as_slice()),
        ([], [Expression::Variable(name), Expression::IntegerLiteral(_)])
            if name == &function.parameters[1].name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn recognizes_a_context_forwarded_after_setup() {
        let function = Function {
            return_type: Type::Void,
            name: "callback".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Short,
                    name: "interrupt".into(),
                },
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 32 },
                    name: "context".into(),
                },
            ],
            locals: vec![],
            statements: vec![
                Statement::Expression(Expression::Call {
                    name: "enable".into(),
                    arguments: vec![],
                }),
                Statement::Expression(Expression::Call {
                    name: "load".into(),
                    arguments: vec![
                        Expression::Variable("context".into()),
                        Expression::IntegerLiteral(1280),
                    ],
                }),
            ],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert!(forwarded_context_wrapper(&function));
    }
}
