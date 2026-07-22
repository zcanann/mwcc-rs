//! Computed locals surviving a call-valued early-return guard.
//!
//! This is a small CFG-liveness owner: it computes one address into a saved
//! home, calls a predicate, and joins the guard result with a load through that
//! address. Keeping the recognition semantic makes the schedule reusable for
//! global object tables without tying it to an SDK symbol or source function.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower `p = &table[index]; if (!predicate(index, C0, C1)) return K;
    /// return p->field;` with `p` in the one callee-saved home.
    pub(crate) fn try_guarded_computed_survivor_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = GuardedComputedSurvivor::recognize(function, self) else {
            return Ok(false);
        };

        let channel = self.lookup_general(shape.index).ok_or_else(|| {
            Diagnostic::error("guarded computed survivor index has no incoming register")
        })?;
        if channel != Eabi::FIRST_GENERAL_ARGUMENT {
            return Ok(false);
        }

        self.non_leaf = true;
        self.callee_saved = vec![31];
        self.epilogue_lr_before_gprs = true;
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                self.frame_size = 24;
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.emit_address_high(4, shape.global);
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                });
                self.emit_guarded_survivor_address_low(shape.global);
                emit_scaled_index(&mut self.output.instructions, 5, channel, shape.stride)?;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -24,
                    });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, shape.slot));
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 20,
                });
                self.output
                    .instructions
                    .push(Instruction::Add { d: 31, a: 0, b: 5 });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(5, shape.frequency));
            }
            FrameConvention::Predecrement => {
                self.frame_size = 16;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -16,
                    });
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.emit_address_high(4, shape.global);
                emit_scaled_index(&mut self.output.instructions, 5, channel, shape.stride)?;
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                });
                self.emit_guarded_survivor_address_low(shape.global);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, shape.slot));
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 12,
                });
                self.output
                    .instructions
                    .push(Instruction::Add { d: 31, a: 0, b: 5 });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(5, shape.frequency));
            }
        }

        self.record_relocation(RelocationKind::Rel24, shape.predicate);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.predicate.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });

        let alternate = self.fresh_label();
        let join = self.fresh_label();
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                self.emit_branch_conditional_to(4, 2, alternate); // bne success
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, shape.guard_result));
                self.emit_branch_to(join);
                self.bind_label(alternate);
                self.output.instructions.push(Instruction::LoadWord {
                    d: 3,
                    a: 31,
                    offset: shape.member_offset,
                });
            }
            FrameConvention::Predecrement => {
                self.emit_branch_conditional_to(12, 2, alternate); // beq guard result
                self.output.instructions.push(Instruction::LoadWord {
                    d: 3,
                    a: 31,
                    offset: shape.member_offset,
                });
                self.emit_branch_to(join);
                self.bind_label(alternate);
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, shape.guard_result));
            }
        }
        self.bind_label(join);
        // The guard's alternate and join are the two optimizer labels that
        // advance the following unwind symbol.
        self.output.anonymous_label_bump += 2;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_guarded_survivor_address_low(&mut self, global: &str) {
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 0,
        });
    }
}

struct GuardedComputedSurvivor<'a> {
    global: &'a str,
    index: &'a str,
    stride: u32,
    predicate: &'a str,
    slot: i16,
    frequency: i16,
    guard_result: i16,
    member_offset: i16,
}

impl<'a> GuardedComputedSurvivor<'a> {
    fn recognize(function: &'a Function, generator: &Generator) -> Option<Self> {
        if !generator.frame_slots.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return None;
        }
        let [local] = function.locals.as_slice() else {
            return None;
        };
        let Type::StructPointer { element_size } = local.declared_type else {
            return None;
        };
        if local.initializer.is_some() || local.is_static || local.array_length.is_some() {
            return None;
        }
        let (name, value, condition, guard_result) =
            match (function.statements.as_slice(), function.guards.as_slice()) {
                (
                    [Statement::Assign { name, value }],
                    [mwcc_syntax_trees::GuardedReturn {
                        condition,
                        value: guard_result,
                    }],
                ) => (name, value, condition, guard_result),
                (
                    [Statement::Assign { name, value }, Statement::If {
                        condition,
                        then_body,
                        else_body,
                    }],
                    [],
                ) if else_body.is_empty() => {
                    let [Statement::Return(Some(guard_result))] = then_body.as_slice() else {
                        return None;
                    };
                    (name, value, condition, guard_result)
                }
                _ => return None,
            };
        if name != &local.name {
            return None;
        }
        let Expression::AddressOf { operand } = value else {
            return None;
        };
        let Expression::Index { base, index } = operand.as_ref() else {
            return None;
        };
        let (Expression::Variable(global), Expression::Variable(index)) =
            (base.as_ref(), index.as_ref())
        else {
            return None;
        };
        if !generator.global_array_sizes.contains_key(global) {
            return None;
        }
        let Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } = condition
        else {
            return None;
        };
        let Expression::Call {
            name: predicate,
            arguments,
        } = operand.as_ref()
        else {
            return None;
        };
        let [Expression::Variable(call_index), slot, frequency] = arguments.as_slice() else {
            return None;
        };
        if call_index != index {
            return None;
        }
        let slot = constant_value(slot).and_then(|value| i16::try_from(value).ok())?;
        let frequency = constant_value(frequency).and_then(|value| i16::try_from(value).ok())?;
        let guard_result =
            constant_value(guard_result).and_then(|value| i16::try_from(value).ok())?;
        let Expression::Member {
            base,
            offset: member_offset,
            member_type,
            ..
        } = function.return_expression.as_ref()?
        else {
            return None;
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == &local.name)
            || !matches!(member_type, Type::Int | Type::UnsignedInt)
        {
            return None;
        }
        Some(Self {
            global,
            index,
            stride: element_size,
            predicate,
            slot,
            frequency,
            guard_result,
            member_offset: i16::try_from(*member_offset).ok()?,
        })
    }
}

fn emit_scaled_index(
    instructions: &mut Vec<Instruction>,
    destination: u8,
    source: u8,
    stride: u32,
) -> Compilation<()> {
    if stride == 1 {
        instructions.push(Instruction::move_register(destination, source));
    } else if stride.is_power_of_two() {
        instructions.push(Instruction::ShiftLeftImmediate {
            a: destination,
            s: source,
            shift: stride.trailing_zeros() as u8,
        });
    } else {
        let immediate = i16::try_from(stride)
            .map_err(|_| Diagnostic::error("guarded computed survivor stride is out of range"))?;
        instructions.push(Instruction::MultiplyImmediate {
            d: destination,
            a: source,
            immediate,
        });
    }
    Ok(())
}
