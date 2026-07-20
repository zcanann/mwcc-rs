//! Two-case local selection followed by a direct-call tail.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

impl Generator {
    /// Lower `switch (short_global) { 0: local=A; 1: local=B; }` followed by a
    /// three-argument call using that local and the incoming parameter, then
    /// argument-free calls. The local lives in r31; the parameter spills because
    /// r3 is needed for the absolute-addressed switch scrutinee.
    pub(crate) fn try_switch_assignment_call_tail(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || self.behavior.schedule_latency_slots
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::Int {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.declared_type != Type::Short
            || local.initializer.is_some()
            || local.array_length.is_some()
            || local.is_static
        {
            return Ok(false);
        }
        let [
            Statement::Switch {
                scrutinee: Expression::Variable(scrutinee),
                arms,
                default: None,
            },
            Statement::Expression(Expression::Call {
                name: first_callee,
                arguments: first_arguments,
            }),
            trailing_calls @ ..,
        ] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if self.globals.get(scrutinee.as_str()) != Some(&Type::Short)
            || arms.len() != 2
            || trailing_calls.is_empty()
            || !trailing_calls.iter().all(|statement| {
                matches!(statement, Statement::Expression(Expression::Call { arguments, .. }) if arguments.is_empty())
            })
        {
            return Ok(false);
        }

        let mut assignments = Vec::with_capacity(2);
        for arm in arms {
            let ArmBody::Statements(statements) = &arm.body else {
                return Ok(false);
            };
            let [Statement::Assign {
                name,
                value: Expression::IntegerLiteral(value),
            }] = statements.as_slice()
            else {
                return Ok(false);
            };
            if name != &local.name
                || arm.falls_through
                || !(i16::MIN as i64..=i16::MAX as i64).contains(value)
            {
                return Ok(false);
            }
            assignments.push((arm.value, *value as i16));
        }
        assignments.sort_by_key(|assignment| assignment.0);
        if assignments[0].0 != 0 || assignments[1].0 != 1 {
            return Ok(false);
        }
        if !matches!(first_arguments.as_slice(), [
            Expression::Variable(selected),
            Expression::Variable(forwarded),
            Expression::IntegerLiteral(-1),
        ] if selected == &local.name && forwarded == &parameter.name)
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            a: 1,
            offset: 8,
        });

        self.emit_address_high(3, scrutinee);
        self.emit_address_low(3, scrutinee);
        self.output
            .instructions
            .push(Instruction::LoadHalfwordAlgebraic {
                d: 0,
                a: 3,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        let beq_one = self.push_switch_branch(12, 2);
        let bge_join = self.push_switch_branch(4, 0);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        let bge_zero = self.push_switch_branch(4, 0);
        let below_zero = self.push_plain_branch();

        let case_zero = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::load_immediate(31, assignments[0].1));
        let zero_join = self.push_plain_branch();
        let case_one = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::load_immediate(31, assignments[1].1));
        let join = self.output.instructions.len();
        self.patch_switch_target(beq_one, case_one);
        self.patch_switch_target(bge_join, join);
        self.patch_switch_target(bge_zero, case_zero);
        self.patch_switch_target(below_zero, join);
        self.patch_switch_target(zero_join, join);

        self.output
            .instructions
            .push(Instruction::ExtendSignHalfword { a: 3, s: 31 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, -1));
        self.emit_direct_tail_call(first_callee);
        for statement in trailing_calls {
            let Statement::Expression(Expression::Call { name, .. }) = statement else {
                unreachable!()
            };
            self.emit_direct_tail_call(name);
        }

        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn push_switch_branch(&mut self, options: u8, condition_bit: u8) -> usize {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        index
    }

    fn push_plain_branch(&mut self) -> usize {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        index
    }

    fn patch_switch_target(&mut self, index: usize, destination: usize) {
        match &mut self.output.instructions[index] {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => *target = destination,
            _ => unreachable!("switch patch points at a non-branch instruction"),
        }
    }

    fn emit_direct_tail_call(&mut self, name: &str) {
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_string(),
        });
    }
}
