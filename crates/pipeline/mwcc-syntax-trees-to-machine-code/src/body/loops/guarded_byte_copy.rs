//! Guarded, count-controlled byte-copy loops.
//!
//! Runtime's `__copy` is a compact control-flow composition rather than a
//! library-name intrinsic: two null/zero guards feed a bottom-tested byte walk.
//! Keeping its recognizer here makes the reusable source shape explicit and
//! leaves the general body driver responsible only for dispatch.

#[allow(unused_imports)]
use super::*;

struct GuardedByteCopy<'a> {
    destination: &'a str,
    source: &'a str,
    count: &'a str,
    cursor: &'a str,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn is_step(expression: &Expression, name: &str, operator: BinaryOperator) -> bool {
    matches!(expression,
        Expression::Binary { operator: found, left, right }
            if *found == operator
                && variable(left) == Some(name)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1)))
}

fn is_nonzero_count(expression: &Expression, name: &str) -> bool {
    variable(expression) == Some(name)
        || matches!(expression,
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            } if variable(left) == Some(name)
                && matches!(right.as_ref(), Expression::IntegerLiteral(0)))
}

fn recognize(function: &Function) -> Option<GuardedByteCopy<'_>> {
    if !function.guards.is_empty() || function_makes_call(function) {
        return None;
    }
    let [destination, source, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(function.return_type, Type::Pointer(_))
        || !matches!(
            destination.parameter_type,
            Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
        )
        || source.parameter_type != destination.parameter_type
        || !matches!(count.parameter_type, Type::Int | Type::UnsignedInt)
        || !matches!(&function.return_expression, Some(expression)
            if variable(expression) == Some(destination.name.as_str()))
    {
        return None;
    }
    let [cursor] = function.locals.as_slice() else {
        return None;
    };
    if cursor.is_static
        || cursor.is_volatile
        || cursor.array_length.is_some()
        || cursor.initializer.is_some()
        || cursor.declared_type != destination.parameter_type
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || variable(left) != Some(destination.name.as_str())
        || variable(right) != Some(count.name.as_str())
    {
        return None;
    }
    let [Statement::Assign {
        name: cursor_assignment,
        value: cursor_value,
    }, Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition: Some(loop_condition),
        step: None,
        body,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if cursor_assignment != &cursor.name
        || variable(cursor_value) != Some(destination.name.as_str())
        || !is_nonzero_count(loop_condition, &count.name)
    {
        return None;
    }
    let [Statement::Store {
        target: Expression::Dereference {
            pointer: store_pointer,
        },
        value: Expression::Dereference {
            pointer: load_pointer,
        },
    }, Statement::Assign {
        name: cursor_step,
        value: cursor_value,
    }, Statement::Assign {
        name: source_step,
        value: source_value,
    }, Statement::Assign {
        name: count_step,
        value: count_value,
    }] = body.as_slice()
    else {
        return None;
    };
    if variable(store_pointer) != Some(cursor.name.as_str())
        || variable(load_pointer) != Some(source.name.as_str())
        || cursor_step != &cursor.name
        || !is_step(cursor_value, &cursor.name, BinaryOperator::Add)
        || source_step != &source.name
        || !is_step(source_value, &source.name, BinaryOperator::Add)
        || count_step != &count.name
        || !is_step(count_value, &count.name, BinaryOperator::Subtract)
    {
        return None;
    }
    Some(GuardedByteCopy {
        destination: &destination.name,
        source: &source.name,
        count: &count.name,
        cursor: &cursor.name,
    })
}

impl Generator {
    /// `if (dst && count) { p = dst; do { *p = *src; ++p; ++src;
    /// --count; } while (count); } return dst;`
    ///
    /// MWCC turns the outer short-circuit into two conditional returns. The
    /// decrement records CR0 and therefore doubles as the loop's bottom test;
    /// source advancement fills the gap before the byte store.
    pub(crate) fn try_guarded_byte_copy(&mut self, function: &Function) -> Compilation<bool> {
        let Some(copy) = recognize(function) else {
            return Ok(false);
        };
        let Some(destination) = self.lookup_general(copy.destination) else {
            return Ok(false);
        };
        let Some(source) = self.lookup_general(copy.source) else {
            return Ok(false);
        };
        let Some(count) = self.lookup_general(copy.count) else {
            return Ok(false);
        };
        let cursor = destination.max(source).max(count) + 1;
        if cursor > 12 || self.frame_slots.contains_key(copy.cursor) {
            return Ok(false);
        }

        // This whole-function recognizer owns a measured instruction schedule.
        self.output.pre_scheduled = true;
        for register in [destination, count] {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: register,
                    immediate: 0,
                });
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: 12,
                    condition_bit: 2,
                });
        }
        self.output
            .instructions
            .push(Instruction::move_register(cursor, destination));
        let loop_body = self.fresh_label();
        self.bind_label(loop_body);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: source,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: count,
                a: count,
                immediate: -1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: source,
            a: source,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: cursor,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: cursor,
            a: cursor,
            immediate: 1,
        });
        self.emit_branch_conditional_to(4, 2, loop_body); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn name(value: &str) -> Expression {
        Expression::Variable(value.to_string())
    }

    fn step(value: &str, operator: BinaryOperator) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(name(value)),
            right: Box::new(Expression::IntegerLiteral(1)),
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Pointer(Pointee::Int),
            name: "copy_bytes".to_string(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Char),
                    name: "dst".to_string(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Char),
                    name: "src".to_string(),
                },
                Parameter {
                    parameter_type: Type::UnsignedInt,
                    name: "n".to_string(),
                },
            ],
            locals: vec![LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Char),
                name: "cursor".to_string(),
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
                condition: Expression::Binary {
                    operator: BinaryOperator::LogicalAnd,
                    left: Box::new(name("dst")),
                    right: Box::new(name("n")),
                },
                then_body: vec![
                    Statement::Assign {
                        name: "cursor".to_string(),
                        value: name("dst"),
                    },
                    Statement::Loop {
                        kind: LoopKind::DoWhile,
                        initializer: None,
                        condition: Some(name("n")),
                        step: None,
                        body: vec![
                            Statement::Store {
                                target: Expression::Dereference {
                                    pointer: Box::new(name("cursor")),
                                },
                                value: Expression::Dereference {
                                    pointer: Box::new(name("src")),
                                },
                            },
                            Statement::Assign {
                                name: "cursor".to_string(),
                                value: step("cursor", BinaryOperator::Add),
                            },
                            Statement::Assign {
                                name: "src".to_string(),
                                value: step("src", BinaryOperator::Add),
                            },
                            Statement::Assign {
                                name: "n".to_string(),
                                value: step("n", BinaryOperator::Subtract),
                            },
                        ],
                    },
                ],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: Some(name("dst")),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_the_shape_independently_of_runtime_symbol_names() {
        let function = function();
        let copy = recognize(&function).unwrap();
        assert_eq!(copy.destination, "dst");
        assert_eq!(copy.source, "src");
        assert_eq!(copy.count, "n");
        assert_eq!(copy.cursor, "cursor");
    }

    #[test]
    fn rejects_a_loop_that_does_not_decrement_its_count() {
        let mut function = function();
        let Statement::If { then_body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::Loop { body, .. } = &mut then_body[1] else {
            unreachable!()
        };
        let Statement::Assign { value, .. } = &mut body[3] else {
            unreachable!()
        };
        *value = step("n", BinaryOperator::Add);
        assert!(recognize(&function).is_none());
    }

    #[test]
    fn accepts_an_explicit_unsigned_greater_than_zero_loop_test() {
        let mut function = function();
        let Statement::If { then_body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::Loop { condition, .. } = &mut then_body[1] else {
            unreachable!()
        };
        *condition = Some(Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(name("n")),
            right: Box::new(Expression::IntegerLiteral(0)),
        });
        assert!(recognize(&function).is_some());
    }
}
