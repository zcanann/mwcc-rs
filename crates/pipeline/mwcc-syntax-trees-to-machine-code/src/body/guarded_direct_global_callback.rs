//! A scalar global callback tested once and invoked with forwarded arguments.
//!
//! An optional constant store belongs to the same scheduling region. Keep its
//! address/value preparation separate from the store and callback load so the
//! existing frame and indirect-call policies can select the measured order.

use super::*;

struct Callback<'a> {
    global: &'a str,
    arguments: &'a [Expression],
    store: Option<(&'a Expression, &'a Expression)>,
}

fn callback_arguments<'a>(call: &'a Expression, global: &str) -> Option<&'a [Expression]> {
    match call {
        Expression::Call { name, arguments } if name == global => Some(arguments),
        Expression::CallThrough { target, arguments } if matches!(target.as_ref(), Expression::Variable(name) if name == global) => {
            Some(arguments)
        }
        _ => None,
    }
}

fn direct_global_callback(function: &Function) -> Option<Callback<'_>> {
    let (store, statements) = match function.statements.as_slice() {
        [Statement::Store { target, value }, rest @ ..] => (Some((target, value)), rest),
        statements => (None, statements),
    };
    let (global, call) = match statements {
        [Statement::If {
            condition,
            then_body,
            else_body,
        }] if else_body.is_empty() => {
            let global = match condition {
                Expression::Variable(global) => global,
                Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left,
                    right,
                } if constant_value(right) == Some(0) => {
                    let Expression::Variable(global) = left.as_ref() else {
                        return None;
                    };
                    global
                }
                _ => return None,
            };
            let [Statement::Expression(call)] = then_body.as_slice() else {
                return None;
            };
            (global, call)
        }
        [Statement::If {
            condition:
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand,
                },
            then_body,
            else_body,
        }, Statement::Expression(call)]
            if else_body.is_empty()
                && matches!(then_body.as_slice(), [Statement::Return(None)]) =>
        {
            let Expression::Variable(global) = operand.as_ref() else {
                return None;
            };
            (global, call)
        }
        _ => return None,
    };
    Some(Callback {
        global,
        arguments: callback_arguments(call, global)?,
        store,
    })
}

enum StoreAddress<'a> {
    SmallData(&'a str),
    Fixed { high: i16, offset: i16, base: u32 },
}

struct PrefixStore<'a> {
    address: StoreAddress<'a>,
    element: Pointee,
    value: i16,
}

impl Generator {
    fn callback_prefix_store<'a>(&self, callback: &Callback<'a>) -> Option<PrefixStore<'a>> {
        let (target, value) = callback.store?;
        let value = i16::try_from(constant_value(value)?).ok()?;
        let (address, element_type) = match target {
            Expression::Variable(global)
                if global != callback.global
                    && self.behavior.global_addressing == GlobalAddressing::SmallData =>
            {
                (StoreAddress::SmallData(global), *self.globals.get(global)?)
            }
            Expression::Index { base, index } => {
                let Expression::Variable(bank) = base.as_ref() else {
                    return None;
                };
                let &(address, element_type) = self.fixed_address_arrays.get(bank)?;
                let (high, low) = split_address(address);
                let element = pointee_of_type(element_type)?;
                let offset = i16::try_from(
                    i64::from(low) + constant_value(index)? * i64::from(element.size()),
                )
                .ok()?;
                let base = (3..12).find(|register| {
                    !self
                        .locations
                        .values()
                        .any(|location| location.register == *register)
                        && !self.reserved.contains(register)
                })?;
                (StoreAddress::Fixed { high, offset, base }, element_type)
            }
            _ => return None,
        };
        let element = pointee_of_type(element_type)?;
        if !matches!(
            element,
            Pointee::Char
                | Pointee::UnsignedChar
                | Pointee::Short
                | Pointee::UnsignedShort
                | Pointee::Int
                | Pointee::UnsignedInt
        ) {
            return None;
        }
        Some(PrefixStore {
            address,
            element,
            value,
        })
    }

    fn emit_callback_prefix_address(&mut self, store: &PrefixStore<'_>) {
        if let StoreAddress::Fixed { high, base, .. } = store.address {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(base, high));
        }
    }

    fn emit_callback_prefix_store(&mut self, store: &PrefixStore<'_>) -> Compilation<()> {
        let (base, offset) = match store.address {
            StoreAddress::SmallData(global) => {
                self.record_relocation(RelocationKind::EmbSda21, global);
                (0, 0)
            }
            StoreAddress::Fixed { base, offset, .. } => (base, offset),
        };
        self.output
            .instructions
            .push(displacement_store(store.element, 0, base, offset)?);
        Ok(())
    }

    pub(super) fn try_guarded_direct_global_callback(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(callback) = direct_global_callback(function) else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(callback.global) else {
            return Ok(false);
        };
        if self.volatile_globals.contains(callback.global)
            || self.variadic_callees.contains(callback.global)
            || callback.arguments.len() > 8
        {
            return Ok(false);
        }
        let parameter_types = self.call_parameter_types.get(callback.global);
        if parameter_types.is_none() && !callback.arguments.is_empty()
            || parameter_types.is_some_and(|types| {
                types.len() != callback.arguments.len()
                    || types.iter().any(|ty| {
                        !matches!(
                            ty,
                            Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                                | Type::Int
                                | Type::UnsignedInt
                                | Type::Pointer(_)
                                | Type::StructPointer { .. }
                        )
                    })
            })
            || callback
                .arguments
                .iter()
                .any(|argument| self.is_float_value(argument))
        {
            return Ok(false);
        }
        // Forwarded leaves already occupy their argument positions. Arbitrary
        // permutations and computed arguments belong to the general marshaler.
        for (index, argument) in callback.arguments.iter().enumerate() {
            let parameter_type =
                parameter_types.expect("nonempty arguments have a signature")[index];
            if let Some(value) = constant_value(argument) {
                let width = parameter_type.width();
                if width < 32 {
                    let (minimum, maximum) = if self.signed_of(parameter_type) {
                        (-(1i64 << (width - 1)), (1i64 << (width - 1)) - 1)
                    } else {
                        (0, (1i64 << width) - 1)
                    };
                    if !(minimum..=maximum).contains(&value) {
                        return Ok(false);
                    }
                }
                continue;
            }
            let Expression::Variable(name) = argument else {
                return Ok(false);
            };
            let Some(location) = self.locations.get(name) else {
                return Ok(false);
            };
            if location.class != ValueClass::General
                || location.width > 32
                || location.width > parameter_type.width()
                || (location.width < 32
                    && location.width == parameter_type.width()
                    && location.signed != self.signed_of(parameter_type))
                || location.register != (index as u8 + 3).into()
            {
                return Ok(false);
            }
        }
        let store = self.callback_prefix_store(&callback);
        if callback.store.is_some() && store.is_none() {
            return Ok(false);
        }
        let tail = self.behavior.terminal_indirect_tail_call;
        let linkage_first = self.behavior.frame_convention == FrameConvention::LinkageFirst;
        self.output.pre_scheduled = true;
        if linkage_first {
            if let Some(PrefixStore {
                address: StoreAddress::SmallData(global),
                ..
            }) = &store
            {
                // The legacy optimizer creates the reused callback load before
                // the independent prefix store, including its symbol identity.
                self.output.symbol_order = vec![callback.global.to_owned(), (*global).to_owned()];
            }
        }
        if !tail {
            self.emit_plain_nonleaf_prologue();
            if let Some(store) = &store {
                // The independent bank high fills the LR-save latency slot.
                if let StoreAddress::Fixed { high, base, .. } = store.address {
                    let slot = if linkage_first { 1 } else { 2 };
                    self.output
                        .instructions
                        .insert(slot, Instruction::load_immediate_shifted(base, high));
                }
                let constant = Instruction::load_immediate(0, store.value);
                if linkage_first {
                    let stack_update = self.output.instructions.len() - 1;
                    self.output.instructions.insert(stack_update, constant);
                } else {
                    self.output.instructions.push(constant);
                }
            }
        } else if let Some(store) = &store {
            if matches!(store.address, StoreAddress::Fixed { .. }) {
                if self.behavior.fixed_address_constant_store_style
                    == FixedAddressConstantStoreStyle::BaseFirst
                {
                    self.emit_callback_prefix_address(store);
                }
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, store.value));
                if self.behavior.fixed_address_constant_store_style
                    == FixedAddressConstantStoreStyle::ValueFirst
                {
                    self.emit_callback_prefix_address(store);
                }
            }
        }
        let store_before_load = store.as_ref().is_some_and(|store| {
            !linkage_first && matches!(store.address, StoreAddress::Fixed { .. })
        });
        if store_before_load {
            self.emit_callback_prefix_store(store.as_ref().unwrap())?;
        }
        self.evaluate(
            &Expression::Variable(callback.global.to_owned()),
            global_type,
            12,
        )?;
        if let Some(store) = &store {
            if tail && matches!(store.address, StoreAddress::SmallData(_)) {
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, store.value));
            }
            if !store_before_load {
                self.emit_callback_prefix_store(store)?;
            }
        }
        self.output.instructions.push(if tail {
            Instruction::CompareWordImmediate {
                a: 12,
                immediate: 0,
            }
        } else {
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            }
        });
        let skip = self.output.instructions.len();
        self.output.instructions.push(if tail {
            Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            }
        } else {
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            }
        });
        if linkage_first {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
        }
        for (index, argument) in callback.arguments.iter().enumerate() {
            if tail && matches!(argument, Expression::Variable(_)) {
                continue;
            }
            self.evaluate(argument, Type::Int, (index as u8 + 3).into())?;
        }
        if linkage_first {
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
        } else {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 12 });
            self.output.instructions.push(if tail {
                Instruction::BranchToCountRegister
            } else {
                Instruction::BranchToCountRegisterAndLink
            });
        }
        let epilogue = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[skip]
        {
            *target = epilogue;
        }
        if linkage_first
            && self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        {
            // Guarded indirect calls fill the LR reload latency with the stack
            // restore. A profile that reloads through the restored stack keeps
            // its ordinary exit below instead.
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: self.frame_size + 4,
                },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: self.frame_size,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]);
        } else {
            self.emit_epilogue_and_return();
        }
        Ok(true)
    }
}

#[cfg(test)]
mod direct_tests {
    use super::*;

    #[test]
    fn retains_the_prefix_store_and_forwarded_callback_arguments() {
        let arguments = vec![
            Expression::IntegerLiteral(0),
            Expression::Variable("context".into()),
        ];
        let function = function(vec![
            Statement::Store {
                target: Expression::Variable("ready".into()),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: Expression::Variable("callback".into()),
                then_body: vec![Statement::Expression(Expression::CallThrough {
                    target: Box::new(Expression::Variable("callback".into())),
                    arguments: arguments.clone(),
                })],
                else_body: Vec::new(),
            },
        ]);
        let callback = direct_global_callback(&function).unwrap();
        assert_eq!(callback.global, "callback");
        assert!(matches!(callback.arguments,
            [Expression::IntegerLiteral(0), Expression::Variable(context)] if context == "context"));
        assert!(
            matches!(callback.store, Some((Expression::Variable(name), value))
            if name == "ready" && constant_value(value) == Some(1))
        );
    }

    #[test]
    fn does_not_reuse_the_guard_value_across_an_intervening_store() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("callback".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Variable("callback".into()),
                    value: Expression::Variable("replacement".into()),
                },
                Statement::Expression(Expression::Call {
                    name: "callback".into(),
                    arguments: Vec::new(),
                }),
            ],
            else_body: Vec::new(),
        }]);
        assert!(direct_global_callback(&function).is_none());
    }

    #[test]
    fn recognizes_early_null_return_before_direct_callback() {
        let function = function(vec![
            Statement::If {
                condition: Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: Box::new(Expression::Variable("callback".into())),
                },
                then_body: vec![Statement::Return(None)],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "callback".into(),
                arguments: Vec::new(),
            }),
        ]);

        assert_eq!(
            direct_global_callback(&function).map(|callback| callback.global),
            Some("callback")
        );
    }

    #[test]
    fn rejects_a_call_to_a_different_global_after_the_guard() {
        let function = function(vec![
            Statement::If {
                condition: Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: Box::new(Expression::Variable("callback".into())),
                },
                then_body: vec![Statement::Return(None)],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "other".into(),
                arguments: Vec::new(),
            }),
        ]);

        assert_eq!(
            direct_global_callback(&function).map(|callback| callback.global),
            None
        );
    }

    #[test]
    fn recognizes_a_bare_global_truth_guard() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("callback".into()),
            then_body: vec![Statement::Expression(Expression::Call {
                name: "callback".into(),
                arguments: Vec::new(),
            })],
            else_body: Vec::new(),
        }]);

        assert_eq!(
            direct_global_callback(&function).map(|callback| callback.global),
            Some("callback")
        );
    }

    #[test]
    fn rejects_a_different_callback_under_a_bare_truth_guard() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("callback".into()),
            then_body: vec![Statement::Expression(Expression::Call {
                name: "other".into(),
                arguments: Vec::new(),
            })],
            else_body: Vec::new(),
        }]);

        assert_eq!(
            direct_global_callback(&function).map(|callback| callback.global),
            None
        );
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "invoke".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }
}
