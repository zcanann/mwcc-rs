//! Leaf constructor-style member-store schedules.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
enum GuardedMemberStoreSource {
    General(u8),
    IntegerZero,
    FloatZero,
}

#[derive(Clone, Copy)]
struct GuardedMemberStore {
    offset: i16,
    pointee: Pointee,
    source: GuardedMemberStoreSource,
}

impl Generator {
    /// Lower constructor-style initialization guarded by a null return:
    /// `if (p == 0) return; p->i = 0; p->q = q; p->f = 0.0f; ...`.
    ///
    /// This is deliberately a whole-sequence matcher. The four measured MWCC
    /// generations disagree about when a shared zero enters f0, and GC/2.0p1
    /// reloads it for every floating store. Generic per-statement lowering loses
    /// that scheduling provenance and cannot reproduce any one rule honestly.
    pub(crate) fn try_guarded_member_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }, tail @ ..] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if tail.len() < 2
            || !matches!(then_body.as_slice(), [Statement::Return(None)])
            || !else_body.is_empty()
        {
            return Ok(false);
        }

        let base_name = match condition {
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => {
                let Expression::Variable(name) = left.as_ref() else {
                    return Ok(false);
                };
                name
            }
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if matches!(left.as_ref(), Expression::IntegerLiteral(0)) => {
                let Expression::Variable(name) = right.as_ref() else {
                    return Ok(false);
                };
                name
            }
            _ => return Ok(false),
        };
        let Some(base_parameter) = function
            .parameters
            .iter()
            .find(|parameter| parameter.name == *base_name)
        else {
            return Ok(false);
        };
        if !matches!(
            base_parameter.parameter_type,
            Type::StructPointer { .. } | Type::Pointer(_)
        ) {
            return Ok(false);
        }
        let Some(base_register) = self.lookup_general(base_name) else {
            return Ok(false);
        };

        let mut stores = Vec::with_capacity(tail.len());
        let mut has_integer_zero = false;
        let mut has_float_zero = false;
        for statement in tail {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } = statement
            else {
                return Ok(false);
            };
            if !matches!(base.as_ref(), Expression::Variable(name) if name == base_name) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(*member_type) else {
                return Ok(false);
            };
            let Ok(offset) = i16::try_from(*offset) else {
                return Ok(false);
            };
            let source = match (pointee, value) {
                (Pointee::Float, Expression::FloatLiteral(value)) if *value == 0.0 => {
                    has_float_zero = true;
                    GuardedMemberStoreSource::FloatZero
                }
                (Pointee::Double, _) | (Pointee::Float, _) => return Ok(false),
                (_, Expression::IntegerLiteral(0)) => {
                    has_integer_zero = true;
                    GuardedMemberStoreSource::IntegerZero
                }
                (_, Expression::Variable(name)) => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    GuardedMemberStoreSource::General(register)
                }
                _ => return Ok(false),
            };
            stores.push(GuardedMemberStore {
                offset,
                pointee,
                source,
            });
        }
        // Keep the matcher scoped to the mixed schedule characterized across
        // all supported builds. Uniform fills belong to their existing paths.
        if !has_integer_zero || !has_float_zero {
            return Ok(false);
        }

        let style = self.behavior.guarded_member_initialization_style;
        let (options, condition_bit) =
            if style == GuardedMemberInitializationStyle::PooledFloatThenInteger {
                // The 4.x optimizer canonicalizes this pointer-null equality as
                // a signed zero test (`cmpwi`); the older lines retain `cmplwi`.
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate {
                        a: base_register,
                        immediate: 0,
                    });
                (4, 2)
            } else {
                self.emit_condition_test(condition)?
            };
        // The return edge is folded into `beqlr` only after MWCC has allocated
        // source-CFG ordinals. The 4.x optimizer retains four slots for this
        // shape; the older lines retain two. Both precede the pooled zero.
        self.output.anonymous_label_bump +=
            if style == GuardedMemberInitializationStyle::PooledFloatThenInteger {
                4
            } else {
                2
            };
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: options ^ 8,
                condition_bit,
            });

        if matches!(
            style,
            GuardedMemberInitializationStyle::IntegerThenPooledFloat
                | GuardedMemberInitializationStyle::ReloadFloatPerStore
                | GuardedMemberInitializationStyle::LazyPooledFloat
        ) {
            self.load_integer_constant(GENERAL_SCRATCH, 0);
        }
        if matches!(
            style,
            GuardedMemberInitializationStyle::IntegerThenPooledFloat
                | GuardedMemberInitializationStyle::PooledFloatThenInteger
        ) {
            self.load_float_literal(FLOAT_SCRATCH, 0.0, false);
        }
        if style == GuardedMemberInitializationStyle::PooledFloatThenInteger {
            self.load_integer_constant(GENERAL_SCRATCH, 0);
        }

        let mut lazy_float_loaded = false;
        for store in stores {
            let source = match store.source {
                GuardedMemberStoreSource::General(register) => register,
                GuardedMemberStoreSource::IntegerZero => GENERAL_SCRATCH,
                GuardedMemberStoreSource::FloatZero => {
                    if style == GuardedMemberInitializationStyle::ReloadFloatPerStore
                        || (style == GuardedMemberInitializationStyle::LazyPooledFloat
                            && !lazy_float_loaded)
                    {
                        self.load_float_literal(FLOAT_SCRATCH, 0.0, false);
                        lazy_float_loaded = true;
                    }
                    FLOAT_SCRATCH
                }
            };
            self.output.instructions.push(displacement_store(
                store.pointee,
                source,
                base_register,
                store.offset,
            )?);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Lower `p->a = value; p->b = C1; p->c = C2;` when `p` and `value` are
    /// incoming integer-class parameters. After the first store, mwcc reuses
    /// the dead value register for C1 and puts C2 in r0, materializing both
    /// constants before issuing their stores.
    pub(crate) fn try_member_parameter_two_constant_fill(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [first, second, third] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: first_base,
                    offset: first_offset,
                    member_type: first_type,
                    index_stride: None,
                },
            value: Expression::Variable(value_name),
        } = first
        else {
            return Ok(false);
        };
        let Expression::Variable(base_name) = first_base.as_ref() else {
            return Ok(false);
        };
        if base_name == value_name {
            return Ok(false);
        }
        let Some(base_parameter) = function
            .parameters
            .iter()
            .find(|parameter| parameter.name == *base_name)
        else {
            return Ok(false);
        };
        if !matches!(
            base_parameter.parameter_type,
            Type::StructPointer { .. } | Type::Pointer(_)
        ) || !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *value_name)
        {
            return Ok(false);
        }

        let member_constant = |statement: &Statement| {
            let Statement::Store {
                target:
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    },
                value,
            } = statement
            else {
                return None;
            };
            let Expression::Variable(name) = base.as_ref() else {
                return None;
            };
            (name == base_name).then_some((*offset, *member_type, constant_value(value)?))
        };
        let Some((second_offset, second_type, second_value)) = member_constant(second) else {
            return Ok(false);
        };
        let Some((third_offset, third_type, third_value)) = member_constant(third) else {
            return Ok(false);
        };
        if second_value == third_value {
            return Ok(false);
        }
        let Some(first_pointee) = pointee_of_type(*first_type) else {
            return Ok(false);
        };
        let Some(second_pointee) = pointee_of_type(second_type) else {
            return Ok(false);
        };
        let Some(third_pointee) = pointee_of_type(third_type) else {
            return Ok(false);
        };
        if matches!(
            (first_pointee, second_pointee, third_pointee),
            (Pointee::Float | Pointee::Double, _, _)
                | (_, Pointee::Float | Pointee::Double, _)
                | (_, _, Pointee::Float | Pointee::Double)
        ) {
            return Ok(false);
        }
        let (first_offset, second_offset, third_offset) = match (
            i16::try_from(*first_offset),
            i16::try_from(second_offset),
            i16::try_from(third_offset),
        ) {
            (Ok(first), Ok(second), Ok(third)) => (first, second, third),
            _ => return Ok(false),
        };
        let base_register = self.general_register_of_leaf(first_base)?;
        let value = Expression::Variable(value_name.clone());
        let value_register = self.general_register_of_leaf(&value)?;
        if base_register == value_register || value_register == GENERAL_SCRATCH {
            return Ok(false);
        }

        self.output.instructions.push(displacement_store(
            first_pointee,
            value_register,
            base_register,
            first_offset,
        )?);
        self.load_integer_constant(value_register, second_value);
        self.load_integer_constant(GENERAL_SCRATCH, third_value);
        self.output.instructions.push(displacement_store(
            second_pointee,
            value_register,
            base_register,
            second_offset,
        )?);
        self.output.instructions.push(displacement_store(
            third_pointee,
            GENERAL_SCRATCH,
            base_register,
            third_offset,
        )?);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
