//! Counted byte-table searches followed by call-based return guards.
//!
//! This owns the Animal Crossing event-dispatch shape. Keeping its structural
//! recognition beside loop lowering prevents the generic call/guard boundary
//! from growing another unrelated special case.

#[allow(unused_imports)]
use super::*;

struct TableSearch<'a> {
    table: &'a str,
    callee: &'a str,
    skip: i16,
    bound: i16,
    fixed_argument: i64,
}

impl Generator {
    /// `for (i=0; i<N; i++) if (i!=SKIP && check(table[i], K)) return i;`
    /// followed by one or more `if (check(C,K)) return R;` guards and a constant
    /// default. With absolute data addressing mwcc walks the byte table in r31,
    /// keeps `i` in r30, and sends every exit to one shared epilogue.
    pub(crate) fn try_counted_table_search_with_call_guards(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || function.return_type != Type::UnsignedChar
            || function.guards.is_empty()
        {
            return Ok(false);
        }
        let Some(shape) = recognize_table_search(function, self) else {
            return Ok(false);
        };
        let Some(default) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .filter(|value| i16::try_from(*value).is_ok())
        else {
            return Ok(false);
        };

        // Every trailing guard calls the same predicate with two literal
        // arguments and returns a small literal. This is the schedule mwcc can
        // thread without preserving any additional live value.
        for guard in &function.guards {
            let Expression::Call { name, arguments } = &guard.condition else {
                return Ok(false);
            };
            if name != shape.callee || arguments.len() != 2 {
                return Ok(false);
            }
            let [first, second] = arguments.as_slice() else {
                return Ok(false);
            };
            if constant_value(first).is_none()
                || constant_value(second) != Some(shape.fixed_argument)
                || constant_value(&guard.value).and_then(|value| i16::try_from(value).ok()).is_none()
            {
                return Ok(false);
            }
        }

        self.emit_counted_table_search_with_call_guards(function, &shape, default as i16)?;
        Ok(true)
    }

    fn emit_counted_table_search_with_call_guards(
        &mut self,
        function: &Function,
        shape: &TableSearch<'_>,
        default: i16,
    ) -> Compilation<()> {
        let table_home = self.fresh_virtual_general();
        let counter_home = self.fresh_virtual_general();
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![table_home, counter_home];
        // The counted search region contributes eight labels, followed by two
        // per trailing guard (measured against extab numbering).
        self.output.anonymous_label_bump = 8 + 2 * function.guards.len() as u32;

        // The scheduler begins the table's absolute address between linkage
        // operations, then completes it directly into the r31 home.
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
        self.emit_address_high(3, shape.table);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: table_home,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: table_home,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: counter_home,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter_home,
            a: 0,
            immediate: 0,
        });

        let loop_top = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: counter_home,
                immediate: shape.skip,
            });
        let skip_candidate = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 3,
            a: table_home,
            offset: 0,
        });
        self.load_integer_constant(4, shape.fixed_argument);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let failed_candidate = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 3,
                s: counter_home,
                clear: 24,
            });
        let mut epilogue_branches = vec![self.output.instructions.len()];
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });

        let increment = self.output.instructions.len();
        for branch in [skip_candidate, failed_candidate] {
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch]
            {
                *target = increment;
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter_home,
            a: counter_home,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: table_home,
            a: table_home,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: counter_home,
                immediate: shape.bound,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_top,
            });

        for (index, guard) in function.guards.iter().enumerate() {
            let Expression::Call { name, arguments } = &guard.condition else {
                unreachable!("recognizer gated every guard to a direct call")
            };
            self.emit_call(name, arguments, None, false)?;
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
            let is_last = index + 1 == function.guards.len();
            if is_last {
                self.load_integer_constant(3, i64::from(default));
                let branch = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options: 12,
                        condition_bit: 2,
                        target: 0,
                    });
                epilogue_branches.push(branch);
                self.load_integer_constant(3, constant_value(&guard.value).unwrap());
            } else {
                let next_guard = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options: 12,
                        condition_bit: 2,
                        target: 0,
                    });
                self.load_integer_constant(3, constant_value(&guard.value).unwrap());
                epilogue_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
                let target = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target: to, .. } =
                    &mut self.output.instructions[next_guard]
                {
                    *to = target;
                }
            }
        }

        let epilogue = self.output.instructions.len();
        for branch in epilogue_branches {
            match &mut self.output.instructions[branch] {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => *target = epilogue,
                _ => unreachable!("recorded only epilogue branches"),
            }
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: table_home,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: counter_home,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(())
    }
}

fn recognize_table_search<'a>(function: &'a Function, generator: &Generator) -> Option<TableSearch<'a>> {
    let [pointer, counter] = function.locals.as_slice() else {
        return None;
    };
    let (Type::Pointer(Pointee::UnsignedChar), Some(Expression::Variable(pointer_table))) =
        (pointer.declared_type, pointer.initializer.as_ref())
    else {
        return None;
    };
    if pointer.is_static || pointer.array_length.is_some() || pointer.data_bytes.is_some() {
        return None;
    }
    if counter.declared_type != Type::Int
        || counter.is_static
        || counter.array_length.is_some()
        || counter.data_bytes.is_some()
        || constant_value(counter.initializer.as_ref()?) != Some(0)
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer, Expression::Assign { target, value }
        if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
            && constant_value(value) == Some(0))
        || !matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                    if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name)
                        && constant_value(right) == Some(1)))
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) {
        return None;
    }
    let bound = i16::try_from(constant_value(right)?).ok()?;
    if bound <= 0 {
        return None;
    }
    let [Statement::If {
        condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Return(Some(Expression::Variable(name)))] if name == &counter.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: skip_test,
        right: call_test,
    } = condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: skip_counter,
        right: skip_value,
    } = skip_test.as_ref()
    else {
        return None;
    };
    if !matches!(skip_counter.as_ref(), Expression::Variable(name) if name == &counter.name) {
        return None;
    }
    let skip = i16::try_from(constant_value(skip_value)?).ok()?;
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: call,
        right: zero,
    } = call_test.as_ref()
    else {
        return None;
    };
    if constant_value(zero) != Some(0) {
        return None;
    }
    let Expression::Call { name: callee, arguments } = call.as_ref() else {
        return None;
    };
    let [Expression::Index { base, index }, fixed] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(table) = base.as_ref() else {
        return None;
    };
    if table != pointer_table
        || !matches!(index.as_ref(), Expression::Variable(name) if name == &counter.name)
        || !matches!(generator.globals.get(table.as_str()), Some(Type::UnsignedChar))
        || generator.global_array_sizes.get(table.as_str()).copied()? < bound as u32
    {
        return None;
    }
    Some(TableSearch {
        table,
        callee,
        skip,
        bound,
        fixed_argument: constant_value(fixed)?,
    })
}
