//! Runtime endian detection through a four-byte stack image.
//!
//! MetroTRK writes `12 34 56 78` to a byte array, reads the image as a word,
//! and publishes whether native order is big- or little-endian. MWCC treats
//! the entire body as one scheduling region: the absolute global base and both
//! comparison operands remain live across the nested branch.

#[allow(unused_imports)]
use super::*;

struct EndianProbe<'a> {
    result: &'a str,
    global: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == expected)
}

fn store_constant(statement: &Statement, target: &str, value: i64) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Variable(found),
        value: expression,
    } if found == target && constant_value(expression) == Some(value))
}

fn byte_store(statement: &Statement, bytes: &str, index: i64, value: i64) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Index { base, index: found_index },
        value: found_value,
    } if variable(base, bytes)
        && constant_value(found_index) == Some(index)
        && constant_value(found_value) == Some(value))
}

fn compares_stack_word(condition: &Expression, bytes: &str, constant: i64) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return false;
    };
    let loaded = |expression: &Expression| {
        matches!(expression, Expression::Dereference { pointer }
            if matches!(pointer.as_ref(), Expression::Cast {
                target_type: Type::Pointer(Pointee::UnsignedInt),
                operand,
            } if variable(operand, bytes)))
    };
    (loaded(left) && constant_value(right) == Some(constant))
        || (loaded(right) && constant_value(left) == Some(constant))
}

fn classify(function: &Function) -> Option<EndianProbe<'_>> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [bytes, result] = function.locals.as_slice() else {
        return None;
    };
    if bytes.declared_type != Type::UnsignedChar
        || bytes.array_length != Some(4)
        || result.declared_type != Type::Int
        || result.initializer.as_ref().and_then(constant_value) != Some(0)
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value, &result.name))
    {
        return None;
    }
    let [initialize, byte0, byte1, byte2, byte3, branch] = function.statements.as_slice()
    else {
        return None;
    };
    let Statement::Store {
        target: Expression::Variable(global),
        value,
    } = initialize
    else {
        return None;
    };
    if constant_value(value) != Some(1)
        || !byte_store(byte0, &bytes.name, 0, 0x12)
        || !byte_store(byte1, &bytes.name, 1, 0x34)
        || !byte_store(byte2, &bytes.name, 2, 0x56)
        || !byte_store(byte3, &bytes.name, 3, 0x78)
    {
        return None;
    }
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = branch
    else {
        return None;
    };
    if !compares_stack_word(condition, &bytes.name, 0x1234_5678)
        || !matches!(then_body.as_slice(), [statement]
            if store_constant(statement, global, 1))
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = else_body.as_slice()
    else {
        return None;
    };
    if !compares_stack_word(condition, &bytes.name, 0x7856_3412)
        || !matches!(then_body.as_slice(), [statement]
            if store_constant(statement, global, 0))
        || !matches!(else_body.as_slice(), [Statement::Assign { name, value }]
            if name == &result.name && constant_value(value) == Some(1))
    {
        return None;
    }
    Some(EndianProbe {
        result: &result.name,
        global,
    })
}

impl Generator {
    pub(crate) fn try_endian_probe(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        // The recognizer requires one four-byte byte array followed only by a
        // register result local. It therefore owns the compact r1+8 frame slot
        // directly, before either generic frame planner can claim the body.
        debug_assert!(function
            .locals
            .iter()
            .any(|local| local.name == plan.result));

        let little = self.fresh_label();
        let invalid = self.fresh_label();
        let done = self.fresh_label();
        self.frame_size = 16;
        self.output.pre_scheduled = true;

        self.record_relocation(RelocationKind::Addr16Ha, plan.global);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.record_relocation(RelocationKind::Addr16Lo, plan.global);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 5,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(6, 1),
            Instruction::StoreWord {
                s: 6,
                a: 5,
                offset: 0,
            },
            Instruction::load_immediate(0, 0x12),
            Instruction::load_immediate(3, 0x34),
            Instruction::StoreByte {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::load_immediate(4, 0x56),
            Instruction::load_immediate(0, 0x78),
            Instruction::StoreByte {
                s: 3,
                a: 1,
                offset: 9,
            },
            Instruction::load_immediate(3, 0),
            Instruction::StoreByte {
                s: 4,
                a: 1,
                offset: 10,
            },
            Instruction::StoreByte {
                s: 0,
                a: 1,
                offset: 11,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 4,
                immediate: -0x1234,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0x5678,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, little); // bne
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 5,
            offset: 0,
        });
        self.emit_branch_to(done);
        self.bind_label(little);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted {
                d: 0,
                a: 4,
                immediate: -0x7856,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0x3412,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, invalid); // bne
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 0,
            },
        ]);
        self.emit_branch_to(done);
        self.bind_label(invalid);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_recognizer_accepts_the_constant_on_either_side() {
        let word = Expression::Dereference {
            pointer: Box::new(Expression::Cast {
                target_type: Type::Pointer(Pointee::UnsignedInt),
                operand: Box::new(Expression::Variable("bytes".into())),
            }),
        };
        for (left, right) in [
            (word.clone(), Expression::IntegerLiteral(0x1234_5678)),
            (Expression::IntegerLiteral(0x1234_5678), word.clone()),
        ] {
            assert!(compares_stack_word(
                &Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                "bytes",
                0x1234_5678,
            ));
        }
    }
}
