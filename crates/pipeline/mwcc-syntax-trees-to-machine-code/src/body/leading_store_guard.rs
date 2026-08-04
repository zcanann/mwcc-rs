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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InlinedLeadingStoreGuardPlan {
    constant_load: usize,
    leading_stores: [usize; 2],
    guarded_reload: usize,
    guarded_store: usize,
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
        if self.preceded_by_asm {
            // An earlier asm definition changes this old optimizer's terminal
            // edge canonicalization for the remainder of the translation
            // unit. It keeps `bne .Lreturn` here; the same source compiled in
            // isolation uses `bnelr`.
            self.preserve_terminal_return_branches = true;
            let target = self.output.instructions.len() + 2;
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: value_register,
            a: address_base,
            offset: low,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Preserve the shared constant when the same source transaction was
    /// exposed by inline expansion inside a larger structured function.
    ///
    /// Ordinary store lowering deliberately uses `r0` for short-lived
    /// constants. The fixed-address guard must also load through `r0`, so that
    /// local choice splits the inlined transaction into two constant ranges.
    /// Build 163 instead keeps the value in the first available volatile home
    /// (normally r5) across the guard. Give the complete value one virtual live
    /// range before allocation; the ordinary allocator can then choose the
    /// measured home without hard-coding a physical register into the stream.
    pub(crate) fn retain_inlined_leading_store_guard_constant(&mut self) {
        if !self.behavior.schedule_latency_slots {
            return;
        }
        while let Some(plan) = inlined_leading_store_guard_plan(&self.output.instructions) {
            let retained = self.fresh_virtual_general_preferring(5);
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[plan.constant_load]
            else {
                unreachable!("the leading constant load was matched")
            };
            *d = retained;
            for index in plan.leading_stores {
                let Instruction::StoreHalfword { s, .. } =
                    &mut self.output.instructions[index]
                else {
                    unreachable!("the leading halfword store was matched")
                };
                *s = retained;
            }
            let Instruction::StoreWord { s, .. } =
                &mut self.output.instructions[plan.guarded_store]
            else {
                unreachable!("the guarded word store was matched")
            };
            *s = retained;
            crate::remove_instruction_retargeting_to_next(self, plan.guarded_reload);
        }
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

fn inlined_leading_store_guard_plan(
    instructions: &[Instruction],
) -> Option<InlinedLeadingStoreGuardPlan> {
    for constant_load in 0..instructions.len().saturating_sub(7) {
        let Instruction::AddImmediate {
            d: constant_register,
            a: 0,
            immediate: constant,
        } = instructions[constant_load]
        else {
            continue;
        };
        let [
            Instruction::StoreHalfword {
                s: first_source,
                a: first_base,
                offset: first_offset,
            },
            Instruction::StoreHalfword {
                s: second_source,
                a: second_base,
                offset: second_offset,
            },
        ] = &instructions[constant_load + 1..constant_load + 3]
        else {
            continue;
        };
        if *first_source != constant_register
            || *second_source != constant_register
            || first_base != second_base
            || second_offset.checked_sub(*first_offset) != Some(2)
        {
            continue;
        }

        // Selection may materialize the fixed bank or the frame aggregate
        // address between the member stores and the guard load. Keep this
        // bounded: the recognizable transaction has at most those two
        // independent latency fillers and no use of the short-lived constant.
        let search_end = (constant_load + 6).min(instructions.len().saturating_sub(5));
        for guard_load in constant_load + 3..=search_end {
            if instructions[constant_load + 3..guard_load]
                .iter()
                .flat_map(mwcc_vreg::register_operands)
                .any(|operand| {
                    operand.class == mwcc_vreg::Class::General
                        && operand.register == constant_register
                })
            {
                break;
            }
            let [
                Instruction::LoadWord {
                    d: loaded,
                    a: fixed_base,
                    offset: fixed_offset,
                },
                Instruction::CompareLogicalWord {
                    a: compared_left,
                    b: compared_right,
                },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target,
                },
                Instruction::AddImmediate {
                    d: reloaded,
                    a: 0,
                    immediate: reloaded_constant,
                },
                Instruction::StoreWord {
                    s: stored,
                    a: stored_base,
                    offset: stored_offset,
                },
            ] = &instructions[guard_load..guard_load + 5]
            else {
                continue;
            };
            let compared_with_distinct_address = (*compared_left == constant_register
                && *compared_right != constant_register)
                || (*compared_right == constant_register
                    && *compared_left != constant_register);
            if *loaded == constant_register
                && *reloaded == constant_register
                && *stored == constant_register
                && *reloaded_constant == constant
                && stored_base == fixed_base
                && stored_offset == fixed_offset
                && compared_with_distinct_address
                && *target == guard_load + 5
            {
                return Some(InlinedLeadingStoreGuardPlan {
                    constant_load,
                    leading_stores: [constant_load + 1, constant_load + 2],
                    guarded_reload: guard_load + 3,
                    guarded_store: guard_load + 4,
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_inlined_context_clear_with_independent_latency_fillers() {
        let instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword {
                s: 0,
                a: 1,
                offset: 424,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 1,
                offset: 426,
            },
            Instruction::load_immediate_shifted(7, -32768),
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 216,
            },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 10,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 7,
                offset: 216,
            },
        ];

        assert_eq!(
            inlined_leading_store_guard_plan(&instructions),
            Some(InlinedLeadingStoreGuardPlan {
                constant_load: 0,
                leading_stores: [1, 2],
                guarded_reload: 8,
                guarded_store: 9,
            })
        );
    }

    #[test]
    fn rejects_a_guard_that_does_not_skip_exactly_the_fixed_store() {
        let mut instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreHalfword {
                s: 0,
                a: 1,
                offset: 424,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 1,
                offset: 426,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 216,
            },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 7,
                offset: 216,
            },
        ];
        assert!(inlined_leading_store_guard_plan(&instructions).is_none());
        let Instruction::BranchConditionalForward { target, .. } = &mut instructions[5] else {
            unreachable!()
        };
        *target = 8;
        assert!(inlined_leading_store_guard_plan(&instructions).is_some());
    }
}
