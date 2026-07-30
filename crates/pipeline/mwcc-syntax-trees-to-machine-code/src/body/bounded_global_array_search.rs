//! Fully unrolled searches over short file-scope scalar arrays.
//!
//! A constant-bound `for` loop with only an equality-return guard is not a
//! runtime loop under MWCC's high optimization modes. It becomes a straight
//! chain of loads, comparisons, and early returns. This owner keeps that
//! topology out of generic loop lowering.

#[allow(unused_imports)]
use super::*;

struct BoundedArraySearch<'a> {
    counter: &'a str,
    global: &'a str,
    needle: &'a Expression,
    bound: usize,
    return_value: i64,
}

impl Generator {
    pub(crate) fn bounded_global_array_search_owns_local(
        &self,
        statement: &Statement,
        local: &str,
    ) -> bool {
        classify(statement, &self.global_array_sizes).is_some_and(|search| search.counter == local)
    }

    pub(crate) fn try_emit_bounded_global_array_return_search(
        &mut self,
        statement: &Statement,
    ) -> Compilation<bool> {
        let Some(search) = classify(statement, &self.global_array_sizes) else {
            return Ok(false);
        };
        let element_type = self.globals.get(search.global).copied().ok_or_else(|| {
            Diagnostic::error("bounded global-array search is missing its element type")
        })?;
        let pointee = pointee_of_type(element_type).ok_or_else(|| {
            Diagnostic::error("bounded global-array search needs scalar elements")
        })?;
        if !matches!(
            pointee,
            Pointee::Int | Pointee::UnsignedInt | Pointee::Pointer
        ) || i16::try_from(search.return_value).is_err()
        {
            return Ok(false);
        }
        let needle = self.general_register_of_leaf(search.needle)?;
        let total_size = self.global_array_sizes[search.global];
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small && search.bound != 1 {
            return Ok(false);
        }

        if small {
            self.emit_global_load(search.global, GENERAL_SCRATCH)?;
            self.emit_bounded_search_comparison(needle, search.return_value, GENERAL_SCRATCH);
            return Ok(true);
        }

        let address = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(address, search.global);
        for index in 0..search.bound {
            let offset = if index == 0 {
                self.record_relocation(RelocationKind::Addr16Lo, search.global);
                0
            } else {
                i16::from(pointee.size())
            };
            if index + 1 == search.bound {
                self.output.instructions.push(Instruction::LoadWord {
                    d: GENERAL_SCRATCH,
                    a: address,
                    offset,
                });
            } else {
                self.output
                    .instructions
                    .push(Instruction::LoadWordWithUpdate {
                        d: GENERAL_SCRATCH,
                        a: address,
                        offset,
                    });
            }
            self.emit_bounded_search_comparison(needle, search.return_value, GENERAL_SCRATCH);
        }
        Ok(true)
    }

    fn emit_bounded_search_comparison(&mut self, needle: u8, return_value: i64, loaded: u8) {
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: needle,
                b: loaded,
            });
        let next = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.load_integer_constant(mwcc_target::Eabi::general_result().number, return_value);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let target = self.output.instructions.len();
        self.patch_forward(next, target);
    }
}

fn classify<'a>(
    statement: &'a Statement,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<BoundedArraySearch<'a>> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    let Expression::Assign {
        target: counter,
        value: initial,
    } = initializer
    else {
        return None;
    };
    let Expression::Variable(counter) = counter.as_ref() else {
        return None;
    };
    if constant_value(initial) != Some(0)
        || !matches!(condition,
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if name == counter)
                && constant_value(right).is_some())
        || !matches!(step,
            Expression::Assign { target, value }
                if matches!(target.as_ref(), Expression::Variable(name) if name == counter)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if matches!(left.as_ref(), Expression::Variable(name) if name == counter)
                        && constant_value(right) == Some(1)))
    {
        return None;
    }
    let Expression::Binary { right: bound, .. } = condition else {
        unreachable!("condition shape was checked")
    };
    let bound = usize::try_from(constant_value(bound)?).ok()?;
    if bound == 0 {
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
    if !else_body.is_empty() {
        return None;
    }
    let [Statement::Return(Some(return_value))] = then_body.as_slice() else {
        return None;
    };
    let return_value = constant_value(return_value)?;
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let (needle, indexed) = match (left.as_ref(), right.as_ref()) {
        (needle, indexed @ Expression::Index { .. }) => (needle, indexed),
        (indexed @ Expression::Index { .. }, needle) => (needle, indexed),
        _ => return None,
    };
    let Expression::Index { base, index } = indexed else {
        unreachable!("indexed side was selected")
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    if !matches!(index.as_ref(), Expression::Variable(name) if name == counter) {
        return None;
    }
    let total_size = *global_array_sizes.get(global)?;
    let bound_bytes = u32::try_from(bound).ok()?.checked_mul(4)?;
    if total_size < bound_bytes {
        return None;
    }
    Some(BoundedArraySearch {
        counter,
        global,
        needle,
        bound,
        return_value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_bound_larger_than_the_global_array() {
        let statement = Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(3)),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body: vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("needle".into())),
                    right: Box::new(Expression::Index {
                        base: Box::new(Expression::Variable("table".into())),
                        index: Box::new(Expression::Variable("i".into())),
                    }),
                },
                then_body: vec![Statement::Return(Some(Expression::IntegerLiteral(1)))],
                else_body: vec![],
            }],
        };
        let sizes = std::collections::HashMap::from([("table".into(), 8)]);
        assert!(classify(&statement, &sizes).is_none());
    }
}
