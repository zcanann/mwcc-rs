//! Shared-value store runs followed by a fixed-address pointer clear guard.
//!
//! Dolphin's context clear routine exposes one cross-statement scheduling region:
//! two member stores share a constant with a later guarded fixed-address store,
//! while the guard loads that same fixed-address pointer. Metrowerks keeps the
//! shared value live and fills its first store latency slot with the address base.

#[allow(unused_imports)]
use super::*;

fn constant_through_casts(mut expression: &Expression) -> Option<i64> {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    constant_value(expression)
}

struct LeadingStore {
    member_type: Type,
    offset: i16,
}

struct LeadingStoreGuardPlan<'a> {
    base_name: &'a str,
    constant: i16,
    leading: [LeadingStore; 2],
    fixed_address: u32,
}

impl Generator {
    /// Emit the measured `p->a=C; p->b=C; if (p == FIXED_PTR) FIXED_PTR=C;`
    /// schedule. The fixed-address declaration stores a pointer word, so the
    /// guard load and guarded clear share one materialized absolute-address base.
    pub(crate) fn try_leading_store_guard(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = self.leading_store_guard_plan(function) else {
            return Ok(false);
        };
        let base = self.lookup_general(plan.base_name).ok_or_else(|| {
            Diagnostic::error("leading-store guard base is not in a general register")
        })?;
        let address_base = self.free_general_excluding(base)?;
        let value_register = self.free_general_excluding_two(base, address_base)?;
        let (high, low) = split_address(plan.fixed_address);
        if high == 0 {
            // The measured scheduler uses a materialized high half as its latency
            // filler. A zero-page address has a different register/order policy.
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::load_immediate(value_register, plan.constant));
        if self.behavior.fixed_address_constant_store_style
            == FixedAddressConstantStoreStyle::ValueFirst
        {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(address_base, high));
        }
        self.emit_planned_member_store(&plan.leading[0], value_register, base)?;
        if self.behavior.fixed_address_constant_store_style
            == FixedAddressConstantStoreStyle::BaseFirst
        {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(address_base, high));
        }
        self.emit_planned_member_store(&plan.leading[1], value_register, base)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: address_base,
            offset: low,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: base,
                b: GENERAL_SCRATCH,
            });
        let (options, condition_bit) = false_branch_bo_bi(BinaryOperator::Equal)
            .expect("equality has a conditional-branch encoding");
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options,
                condition_bit,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: value_register,
            a: address_base,
            offset: low,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_planned_member_store(
        &mut self,
        store: &LeadingStore,
        value_register: u8,
        base_register: u8,
    ) -> Compilation<()> {
        let pointee = pointee_of_type(store.member_type).ok_or_else(|| {
            Diagnostic::error("leading-store guard member has no scalar store width")
        })?;
        self.output.instructions.push(displacement_store(
            pointee,
            value_register,
            base_register,
            store.offset,
        )?);
        Ok(())
    }

    fn leading_store_guard_plan<'a>(
        &self,
        function: &'a Function,
    ) -> Option<LeadingStoreGuardPlan<'a>> {
        if function.return_type != Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function_makes_call(function)
        {
            return None;
        }
        let [first, second, Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return None;
        };
        if !else_body.is_empty() {
            return None;
        }

        let parse_member_store = |statement: &'a Statement| {
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
            let Expression::Variable(base_name) = base.as_ref() else {
                return None;
            };
            let constant = i16::try_from(constant_through_casts(value)?).ok()?;
            Some((
                base_name.as_str(),
                constant,
                LeadingStore {
                    member_type: *member_type,
                    offset: i16::try_from(*offset).ok()?,
                },
            ))
        };
        let (base_name, constant, first) = parse_member_store(first)?;
        let (second_base, second_constant, second) = parse_member_store(second)?;
        if second_base != base_name || second_constant != constant {
            return None;
        }

        let Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } = condition
        else {
            return None;
        };
        if !matches!(left.as_ref(), Expression::Variable(name) if name == base_name) {
            return None;
        }
        let Expression::Dereference { pointer } = right.as_ref() else {
            return None;
        };
        let (loaded_pointee, fixed_address) = const_address_pointer(pointer)?;
        if !matches!(loaded_pointee, Pointee::Pointer | Pointee::WordPointer)
            || !self
                .fixed_address_objects
                .values()
                .any(|address| *address == fixed_address)
        {
            return None;
        }

        let [Statement::Store {
            target: Expression::Dereference { pointer },
            value,
        }] = then_body.as_slice()
        else {
            return None;
        };
        let (stored_pointee, stored_address) = const_address_pointer(pointer)?;
        if stored_address != fixed_address
            || !matches!(stored_pointee, Pointee::Pointer | Pointee::WordPointer)
            || i16::try_from(constant_through_casts(value)?).ok()? != constant
        {
            return None;
        }

        Some(LeadingStoreGuardPlan {
            base_name,
            constant,
            leading: [first, second],
            fixed_address,
        })
    }
}
