//! Range-guarded global-array elements consumed by a sequence of calls.
//!
//! The selected element address is the only value that survives the calls, so
//! it receives one virtual callee-saved home. The ordered conjunction remains
//! explicit because legacy mwcc preserves each source comparison and false edge.

#[allow(unused_imports)]
use super::*;

struct GuardComparison {
    immediate: i16,
    false_options: u8,
    condition_bit: u8,
}

struct GuardedCall<'a> {
    name: &'a str,
    constant: Option<i16>,
}

struct GuardedIndexedCallSequence<'a> {
    index: &'a str,
    element: &'a str,
    array: &'a str,
    stride: i16,
    comparisons: Vec<GuardComparison>,
    calls: Vec<GuardedCall<'a>>,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn flatten_and<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = expression
    {
        flatten_and(left, terms);
        flatten_and(right, terms);
    } else {
        terms.push(expression);
    }
}

fn classify_comparison(expression: &Expression, index: &str) -> Option<GuardComparison> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if !variable(left, index) {
        return None;
    }
    let immediate = i16::try_from(constant_value(right)?).ok()?;
    let (false_options, condition_bit) = match operator {
        BinaryOperator::NotEqual => (12, 2),     // false: equal
        BinaryOperator::GreaterEqual => (12, 0), // false: less
        BinaryOperator::Less => (4, 0),          // false: greater/equal
        _ => return None,
    };
    Some(GuardComparison {
        immediate,
        false_options,
        condition_bit,
    })
}

fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<GuardedIndexedCallSequence<'a>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [index] = function.parameters.as_slice() else {
        return None;
    };
    let [element] = function.locals.as_slice() else {
        return None;
    };
    if index.parameter_type != Type::Int
        || !matches!(
            element.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
        || element.initializer.is_some()
        || element.array_length.is_some()
        || element.is_static
        || element.is_volatile
    {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() || then_body.len() < 3 {
        return None;
    }
    let mut terms = Vec::new();
    flatten_and(condition, &mut terms);
    let comparisons: Vec<_> = terms
        .into_iter()
        .map(|term| classify_comparison(term, &index.name))
        .collect::<Option<_>>()?;
    if comparisons.is_empty() {
        return None;
    }

    let Statement::Assign {
        name: assigned_element,
        value: Expression::AddressOf { operand: selected },
    } = &then_body[0]
    else {
        return None;
    };
    let Expression::Index {
        base: selected_array,
        index: selected_index,
    } = selected.as_ref()
    else {
        return None;
    };
    let Expression::Variable(array) = selected_array.as_ref() else {
        return None;
    };
    if assigned_element != &element.name
        || !variable(selected_index, &index.name)
        || !global_array_sizes.contains_key(array)
    {
        return None;
    }
    let stride = match globals.get(array) {
        Some(Type::Struct { size, .. }) => i16::try_from(*size).ok()?,
        _ => return None,
    };
    if stride <= 0 {
        return None;
    }

    let mut calls = Vec::with_capacity(then_body.len() - 1);
    for statement in &then_body[1..] {
        let Statement::Expression(Expression::Call { name, arguments }) = statement else {
            return None;
        };
        let [first, rest @ ..] = arguments.as_slice() else {
            return None;
        };
        if !variable(first, &element.name) || rest.len() > 1 {
            return None;
        }
        let constant = match rest {
            [] => None,
            [value] => Some(i16::try_from(constant_value(value)?).ok()?),
            _ => return None,
        };
        calls.push(GuardedCall {
            name: name.as_str(),
            constant,
        });
    }
    Some(GuardedIndexedCallSequence {
        index: &index.name,
        element: &element.name,
        array,
        stride,
        comparisons,
        calls,
    })
}

impl Generator {
    /// Lower a guarded `element = &array[index]` followed by calls which all
    /// consume that element. The address is formed once and survives in a
    /// virtual home allocated from the callee-saved pool.
    pub(crate) fn try_guarded_indexed_call_sequence(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function, &self.globals, &self.global_array_sizes) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let index = self.general_register_of(shape.index)?;
        if index != Eabi::FIRST_GENERAL_ARGUMENT {
            return Ok(false);
        }
        let element = self.fresh_virtual_general_preferring(31);
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![element];
        self.output.pre_scheduled = true;

        let done = self.fresh_label();
        let first = &shape.comparisons[0];
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: index,
                immediate: first.immediate,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: element,
            a: 1,
            offset: 12,
        });
        self.emit_branch_conditional_to(first.false_options, first.condition_bit, done);
        for comparison in &shape.comparisons[1..] {
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate {
                    a: index,
                    immediate: comparison.immediate,
                });
            self.emit_branch_conditional_to(
                comparison.false_options,
                comparison.condition_bit,
                done,
            );
        }

        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 4,
                a: index,
                immediate: shape.stride,
            });
        self.record_relocation(RelocationKind::Addr16Ha, shape.array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: Eabi::general_result().number,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, shape.array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: Eabi::general_result().number,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: element,
            a: GENERAL_SCRATCH,
            b: 4,
        });

        for (call_index, call) in shape.calls.iter().enumerate() {
            if call_index + 1 == shape.calls.len() {
                self.output
                    .instructions
                    .push(Instruction::move_register(3, element));
            } else {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: element,
                    immediate: 0,
                });
            }
            if let Some(constant) = call.constant {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(4, constant));
            }
            self.record_relocation(RelocationKind::Rel24, call.name);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: call.name.to_string(),
            });
        }

        self.bind_label(done);
        self.output.instructions.push(Instruction::LoadWord {
            d: element,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.locations.insert(
            shape.element.to_string(),
            Location {
                class: ValueClass::General,
                register: element,
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(shape.stride as u32),
            },
        );
        Ok(true)
    }
}
