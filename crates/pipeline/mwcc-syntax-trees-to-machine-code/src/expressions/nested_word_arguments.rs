//! Compose call-bearing word arguments before assigning their ABI registers.
//!
//! Argument values have virtual homes, so allocation sees values live across
//! nested calls. Arithmetic still uses the ordinary typed expression selector.
use super::*;

fn word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

pub(crate) fn shared_computed_global(arguments: &[Expression], name: &str) -> bool {
    arguments
        .iter()
        .filter(|arg| expression_reads_name(arg, name))
        .count()
        >= 2
        && arguments.iter().any(|arg| {
            let mut value = arg;
            while let Expression::Cast { operand, .. } = value {
                value = operand;
            }
            matches!(value, Expression::Binary { .. })
                && expression_has_call(value)
                && expression_reads_name(value, name)
        })
}

impl Generator {
    fn nested_word_expression(&self, expression: &Expression) -> bool {
        match expression {
            Expression::StringLiteral(_) => true,
            Expression::Call { name, arguments } => {
                self.call_return_types.get(name).copied().is_some_and(word)
                    && !self.globals.contains_key(name)
                    && !self.locations.contains_key(name)
                    && arguments
                        .iter()
                        .all(|argument| self.nested_word_expression(argument))
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                !matches!(
                    operator,
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                ) && self.nested_word_expression(left)
                    && self.nested_word_expression(right)
            }
            Expression::Cast {
                target_type,
                operand,
            } => word(*target_type) && self.nested_word_expression(operand),
            Expression::Unary {
                operator: UnaryOperator::Negate | UnaryOperator::BitNot | UnaryOperator::LogicalNot,
                operand,
            } => self.nested_word_expression(operand),
            other => self.call_input_is_word_expression(other),
        }
    }

    pub(super) fn try_nested_word_arguments(
        &mut self,
        arguments: &[Expression],
        callee: &str,
    ) -> Compilation<bool> {
        if !(2..=8).contains(&arguments.len())
            || !arguments.iter().skip(1).any(expression_has_call)
            || self.globals.contains_key(callee)
            || self.locations.contains_key(callee)
            || matches!(
                self.call_return_types.get(callee),
                Some(Type::Struct { .. })
            )
            || arguments.iter().enumerate().any(|(index, argument)| {
                !self
                    .call_parameter_types
                    .get(callee)
                    .and_then(|types| types.get(index))
                    .copied()
                    .is_none_or(word)
                    || !self.nested_word_expression(argument)
            })
        {
            return Ok(false);
        }
        let mut trial = self.clone();
        let locations = trial.locations.clone();
        let reserved = trial.reserved.clone();
        // Preserve source register bindings before the first nested call. Memory
        // operands stay at their measured evaluation point rather than being
        // captured speculatively at the start of the argument list.
        let mut inputs: Vec<_> = arguments
            .iter()
            .flat_map(|arg| trial.registers_used_by(arg))
            .collect();
        inputs.sort_unstable();
        inputs.dedup();
        for input in inputs {
            let home = trial.fresh_virtual_general();
            trial
                .output
                .instructions
                .push(Instruction::move_register(home, input));
            for location in trial.locations.values_mut() {
                if location.class == ValueClass::General && location.register == input {
                    location.register = home;
                }
            }
            trial.reserved.insert(home);
        }
        let mut arguments = arguments.to_vec();
        if trial.behavior.optimization != mwcc_versions::Optimization::O0 {
            let mut shared: Vec<_> = trial
                .globals
                .iter()
                .filter(|(name, ty)| {
                    word(**ty)
                        && !trial.volatile_globals.contains(*name)
                        && !trial.locations.contains_key(*name)
                        && !trial.global_arrays.contains(*name)
                        && shared_computed_global(&arguments, name)
                })
                .map(|(name, _)| name.clone())
                .collect();
            shared.sort();
            for global in shared {
                let alias = trial.nested_word_alias(&Expression::Variable(global.clone()))?;
                for argument in &mut arguments {
                    *argument = crate::body::rewrite_structured_expression(argument, &mut |e| {
                        matches!(e, Expression::Variable(name) if name == &global)
                            .then(|| Expression::Variable(alias.clone()))
                    });
                }
            }
        }
        let mut homes = vec![0; arguments.len()];
        let order = (0..arguments.len())
            .rev()
            .filter(|&index| constant_value(&arguments[index]).is_none())
            .chain(
                (0..arguments.len()).filter(|&index| constant_value(&arguments[index]).is_some()),
            );
        for index in order {
            let home = trial.fresh_virtual_general();
            trial.emit_nested_word_value(&arguments[index], home)?;
            trial.reserved.insert(home);
            homes[index] = home;
        }
        for (index, home) in homes.into_iter().enumerate() {
            trial
                .output
                .instructions
                .push(Instruction::move_register(3 + index as u32, home));
        }
        trial.locations = locations;
        trial.reserved = reserved;
        *self = trial;
        Ok(true)
    }

    fn nested_pointer_shape(&self, expression: &Expression) -> (Option<Pointee>, Option<u32>) {
        let from_type = |ty: Type| match ty {
            Type::Pointer(pointee) => (Some(pointee), None),
            Type::StructPointer { element_size } => (None, Some(element_size)),
            _ => (None, None),
        };
        match expression {
            Expression::Cast { target_type, .. }
            | Expression::Member {
                member_type: target_type,
                ..
            } => from_type(*target_type),
            Expression::Call { name, .. } => self
                .call_return_types
                .get(name)
                .map_or((None, None), |ty| from_type(*ty)),
            Expression::Variable(name) => self
                .locations
                .get(name)
                .map(|l| (l.pointee, l.stride))
                .unwrap_or_else(|| {
                    self.globals
                        .get(name)
                        .map_or((None, None), |ty| from_type(*ty))
                }),
            Expression::Binary {
                operator: BinaryOperator::Add | BinaryOperator::Subtract,
                left,
                right,
            } => {
                let a = self.nested_pointer_shape(left);
                let b = self.nested_pointer_shape(right);
                match (a != (None, None), b != (None, None)) {
                    (true, false) => a,
                    (false, true)
                        if matches!(
                            expression,
                            Expression::Binary {
                                operator: BinaryOperator::Add,
                                ..
                            }
                        ) =>
                    {
                        b
                    }
                    _ => (None, None),
                }
            }
            _ => (None, None),
        }
    }

    fn nested_word_alias(&mut self, expression: &Expression) -> Compilation<String> {
        let signed = self.signedness_of(expression)?;
        let (pointee, stride) = self.nested_pointer_shape(expression);
        let home = self.fresh_virtual_general();
        self.emit_nested_word_value(expression, home)?;
        let mut name = format!("__mwcc_nested_word_{home}");
        while self.locations.contains_key(&name)
            || self.globals.contains_key(&name)
            || self.known_locals.contains(&name)
        {
            name.push('_');
        }
        self.locations.insert(
            name.clone(),
            Location {
                class: ValueClass::General,
                register: home,
                signed,
                width: 32,
                pointee,
                stride,
            },
        );
        self.reserved.insert(home);
        Ok(name)
    }

    fn emit_nested_word_value(
        &mut self,
        expression: &Expression,
        destination: u32,
    ) -> Compilation<()> {
        if !expression_has_call(expression) {
            return self.evaluate_general(expression, destination);
        }
        let mut aliases = Vec::new();
        let rewritten = match expression {
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                let (a, b) = if expression_has_call(right)
                    && (!expression_has_call(left) || *operator != BinaryOperator::Subtract)
                {
                    let b = self.nested_word_alias(right)?;
                    (self.nested_word_alias(left)?, b)
                } else {
                    let a = self.nested_word_alias(left)?;
                    (a, self.nested_word_alias(right)?)
                };
                aliases.extend([a.clone(), b.clone()]);
                Expression::Binary {
                    operator: *operator,
                    left: Box::new(Expression::Variable(a)),
                    right: Box::new(Expression::Variable(b)),
                }
            }
            Expression::Cast {
                target_type,
                operand,
            } => {
                let name = self.nested_word_alias(operand)?;
                aliases.push(name.clone());
                Expression::Cast {
                    target_type: *target_type,
                    operand: Box::new(Expression::Variable(name)),
                }
            }
            Expression::Unary { operator, operand } => {
                let name = self.nested_word_alias(operand)?;
                aliases.push(name.clone());
                Expression::Unary {
                    operator: *operator,
                    operand: Box::new(Expression::Variable(name)),
                }
            }
            _ => expression.clone(),
        };
        let result = self.evaluate_general(&rewritten, destination);
        for name in aliases {
            let location = self.locations.remove(&name).expect("scoped call value");
            self.reserved.remove(&location.register);
        }
        result
    }
}
