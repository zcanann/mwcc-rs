//! Address-taken local initialization followed by a callee-saved branch.
//!
//! The frame-resident local and the live-in parameter belong to different
//! storage classes, but share one function frame. This owner composes those
//! allocations instead of teaching either expression lowering or the generic
//! frame path about a particular control-flow shape.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower `T out = C; read(..., &out); if (out) arm(saved); else arm(saved);`.
    /// The local occupies r1+8 and the sole live-in survivor occupies a virtual
    /// callee-saved home. Mainline's combined frame is 32 bytes and schedules
    /// the initial slot store between the address calculation and the first call.
    pub(crate) fn try_frame_call_then_branch(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.is_static
            || local.array_length.is_some()
            || local.data_bytes.is_some()
            || local.declared_type != Type::Int
        {
            return Ok(false);
        }
        let Some(initial_value) = local
            .initializer
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let [Statement::Expression(first_call @ Expression::Call { arguments, .. }), Statement::If {
            condition: Expression::Variable(condition),
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if condition != &local.name
            || !arguments.iter().any(|argument| {
                matches!(argument,
                    Expression::AddressOf { operand }
                        if matches!(operand.as_ref(), Expression::Variable(name) if name == &local.name))
            })
            || then_body.is_empty()
            || else_body.is_empty()
            || then_body.iter().chain(else_body).any(|statement| {
                !matches!(statement, Statement::Expression(Expression::Call { .. }))
            })
        {
            return Ok(false);
        }

        let arms_read = |name: &str| {
            then_body.iter().chain(else_body).any(|statement| {
                matches!(statement, Statement::Expression(expression)
                    if expression_reads_name(expression, name))
            })
        };
        let survivors: Vec<&mwcc_syntax_trees::Parameter> = function
            .parameters
            .iter()
            .filter(|parameter| {
                !expression_reads_name(first_call, &parameter.name) && arms_read(&parameter.name)
            })
            .collect();
        let [survivor] = survivors.as_slice() else {
            return Ok(false);
        };
        let Some(location) = self.locations.get(&survivor.name) else {
            return Ok(false);
        };
        if location.class != ValueClass::General {
            return Ok(false);
        }
        let incoming = location.register;

        const LOCAL_OFFSET: i16 = 8;
        self.frame_slots.insert(
            local.name.clone(),
            FrameSlot {
                offset: LOCAL_OFFSET,
                class: ValueClass::General,
                size: 4,
                parameter_register: None,
                is_array: false,
            },
        );
        self.written_slots.insert(LOCAL_OFFSET);
        self.non_leaf = true;
        self.frame_size = 32;
        let home = self.fresh_virtual_general();
        self.callee_saved = vec![home];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: self.frame_size + 4,
        });
        self.load_integer_constant(0, i64::from(initial_value));
        self.output.instructions.push(Instruction::StoreWord {
            s: home,
            a: 1,
            offset: self.frame_size - 4,
        });
        self.emit_callee_saved_home_copy(home, incoming);
        if let Some(location) = self.locations.get_mut(&survivor.name) {
            location.register = home;
        }

        let call_start = self.output.instructions.len();
        self.emit_statement(&function.statements[0])?;
        let call_index = self.output.instructions[call_start..]
            .iter()
            .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|index| call_start + index)
            .ok_or_else(|| Diagnostic::error("frame call-then-branch lost its first call"))?;
        self.output.instructions.insert(
            call_index,
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: LOCAL_OFFSET,
            },
        );
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= call_index {
                relocation.instruction_index += 1;
            }
        }

        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: LOCAL_OFFSET,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        let alternate = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, alternate);
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        self.emit_branch_to(join);
        self.bind_label(alternate);
        for statement in else_body {
            self.emit_statement(statement)?;
        }
        self.bind_label(join);
        self.epilogue_lr_before_gprs = true;
        self.emit_epilogue_and_return();
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}
