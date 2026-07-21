//! Fixed-address vector copies followed by cache maintenance.
//!
//! Dolphin's exception-vector installers keep the destination address live in a
//! callee-saved register across three calls.  The first call also computes a
//! linker-symbol range.  MWCC overlaps the destination and range materialization,
//! so this complete transaction owns their shared schedule.

use super::*;

struct CopyBarrier<'a> {
    address: u32,
    start: &'a str,
    end: &'a str,
    copy: &'a str,
    flush: &'a str,
    invalidate: &'a str,
    size: i16,
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn constant_through_casts(expression: &Expression) -> Option<i64> {
    match peel_casts(expression) {
        Expression::IntegerLiteral(value) => Some(*value),
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left = constant_through_casts(left)? as i32;
            let right = constant_through_casts(right)? as i32;
            Some(match operator {
                BinaryOperator::Add => left.wrapping_add(right),
                BinaryOperator::Subtract => left.wrapping_sub(right),
                BinaryOperator::BitOr => left | right,
                BinaryOperator::ShiftLeft if (0..32).contains(&right) => {
                    left.wrapping_shl(right as u32)
                }
                _ => return None,
            } as i64)
        }
        _ => None,
    }
}

fn direct_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn symbol_address(expression: &Expression) -> Option<&str> {
    match peel_casts(expression) {
        Expression::AddressOf { operand } => match peel_casts(operand) {
            Expression::Variable(name) => Some(name),
            _ => None,
        },
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn recognize(function: &Function) -> Option<CopyBarrier<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [destination] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(
        destination.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || destination.is_static
        || destination.array_length.is_some()
    {
        return None;
    }
    let address = constant_through_casts(destination.initializer.as_ref()?)? as u32;

    let [copy_statement, flush_statement, barrier_statement, invalidate_statement] =
        function.statements.as_slice()
    else {
        return None;
    };
    let (copy, copy_arguments) = direct_call(copy_statement)?;
    let (flush, flush_arguments) = direct_call(flush_statement)?;
    let (barrier, barrier_arguments) = direct_call(barrier_statement)?;
    let (invalidate, invalidate_arguments) = direct_call(invalidate_statement)?;
    let [Expression::Variable(copy_destination), start_argument, range] = copy_arguments else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: range_end,
        right: range_start,
    } = peel_casts(range)
    else {
        return None;
    };
    let start = symbol_address(start_argument)?;
    let end = symbol_address(range_end)?;
    let repeated_start = symbol_address(range_start)?;
    let [Expression::Variable(flush_destination), flush_size] = flush_arguments else {
        return None;
    };
    let [Expression::Variable(invalidate_destination), invalidate_size] = invalidate_arguments
    else {
        return None;
    };
    let size = constant_through_casts(flush_size).and_then(|value| i16::try_from(value).ok())?;
    if copy_destination != &destination.name
        || flush_destination != &destination.name
        || invalidate_destination != &destination.name
        || repeated_start != start
        || constant_through_casts(invalidate_size) != Some(i64::from(size))
        || barrier != "__sync"
        || !barrier_arguments.is_empty()
    {
        return None;
    }

    Some(CopyBarrier {
        address,
        start,
        end,
        copy,
        flush,
        invalidate,
        size,
    })
}

impl Generator {
    pub(crate) fn try_fixed_address_copy_barrier(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.schedule_latency_slots
        {
            return Ok(false);
        }
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        let (address_high, address_low) = split_address(shape.address);

        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![31];
        self.output.pre_scheduled = true;
        // These calls are registered in transaction order.  The generic AST
        // symbol walk groups the two cache operations ahead of the range copy,
        // which is not the legacy compiler's creation order for this schedule.
        self.output.symbol_order = [shape.copy, shape.flush, shape.invalidate]
            .into_iter()
            .map(str::to_owned)
            .collect();
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::load_immediate_shifted(5, address_high),
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, shape.start);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, shape.end);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 5,
            immediate: address_low,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.end);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.start);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 5, a: 4, b: 0 });
        self.record_relocation(RelocationKind::Rel24, shape.copy);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.copy.to_string(),
        });
        self.output.instructions.extend([
            Instruction::move_register(3, 31),
            Instruction::load_immediate(4, shape.size),
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.flush);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.flush.to_string(),
        });
        self.output.instructions.push(Instruction::Synchronize);
        self.output.instructions.extend([
            Instruction::move_register(3, 31),
            Instruction::load_immediate(4, shape.size),
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.invalidate);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.invalidate.to_string(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> Expression {
        Expression::Variable(value.to_string())
    }

    fn call(name: &str, arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.to_string(),
            arguments,
        })
    }

    fn address(name: &str) -> Expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(Expression::AddressOf {
                operand: Box::new(self::name(name)),
            }),
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Void,
            name: "install_vector".to_string(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Int),
                name: "destination".to_string(),
                initializer: Some(Expression::Cast {
                    target_type: Type::Pointer(Pointee::Int),
                    operand: Box::new(Expression::IntegerLiteral(0x8000_0c00)),
                }),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![
                call(
                    "copy_range",
                    vec![
                        name("destination"),
                        name("vector_begin"),
                        Expression::Binary {
                            operator: BinaryOperator::Subtract,
                            left: Box::new(address("vector_end")),
                            right: Box::new(address("vector_begin")),
                        },
                    ],
                ),
                call(
                    "flush_data",
                    vec![name("destination"), Expression::IntegerLiteral(256)],
                ),
                call("__sync", Vec::new()),
                call(
                    "invalidate_code",
                    vec![name("destination"), Expression::IntegerLiteral(256)],
                ),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: true,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_equivalent_names_and_calls() {
        let function = function();
        let shape = recognize(&function).expect("semantic shape");
        assert_eq!(shape.address, 0x8000_0c00);
        assert_eq!(shape.start, "vector_begin");
        assert_eq!(shape.end, "vector_end");
        assert_eq!(shape.copy, "copy_range");
    }

    #[test]
    fn rejects_mismatched_range_start() {
        let mut function = function();
        let Statement::Expression(Expression::Call { arguments, .. }) = &mut function.statements[0]
        else {
            unreachable!()
        };
        let Expression::Binary { right, .. } = &mut arguments[2] else {
            unreachable!()
        };
        *right = Box::new(address("different_begin"));
        assert!(recognize(&function).is_none());
    }
}
