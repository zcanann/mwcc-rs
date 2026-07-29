//! Legacy two-word tests of a zero-extended call result.
//!
//! Early PowerPC builds retain the declared `u64` value graph even when a
//! 32-bit call defines the value and every observed mask is in its low word.
//! The high zero word and low call result are therefore tested as a pair. This
//! owner recognizes that complete semantic transaction so the ordinary
//! one-register expression evaluator never silently narrows it.

#[allow(unused_imports)]
use super::*;

struct WideMaskChain {
    global: String,
    callee: String,
    masks: [i16; 2],
    guarded_values: [i16; 2],
    default_value: i16,
}

impl Generator {
    pub(crate) fn try_wide_call_result_mask_chain(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let Some(plan) = recognize(function, &self.globals, &self.call_return_types) else {
            return Ok(false);
        };
        if self.behavior.wide_call_result_mask_style
            == mwcc_versions::WideCallResultMaskStyle::ScalarizeLowWord
        {
            let Some(masks) = plan
                .masks
                .map(|mask| crate::analysis::rlwinm_mask(i64::from(mask)))
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .and_then(|masks| <[(u8, u8); 2]>::try_from(masks).ok())
            else {
                return Ok(false);
            };
            self.emit_scalarized_wide_mask_chain(&plan, masks);
            return Ok(true);
        }
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 8;
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        // The retained high/low value graph consumes four optimizer ordinals
        // before the function's exception-table anchor.
        self.output.anonymous_label_bump += 4;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.clone(),
        });

        const HIGH_ZERO: u8 = 6;
        const HIGH_MASKED: u8 = 5;
        self.output
            .instructions
            .push(Instruction::load_immediate(HIGH_ZERO, 0));
        self.record_relocation_with_addend(RelocationKind::EmbSda21, &plan.global, 4);
        self.output.instructions.push(Instruction::StoreWord {
            s: Eabi::general_result().number,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, plan.masks[0]));
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: Eabi::general_result().number,
            b: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, &plan.global);
        self.output.instructions.push(Instruction::StoreWord {
            s: HIGH_ZERO,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::And {
            a: HIGH_MASKED,
            s: HIGH_ZERO,
            b: HIGH_ZERO,
        });
        self.output.instructions.push(Instruction::Xor {
            a: 4,
            s: 0,
            b: HIGH_ZERO,
        });
        self.output.instructions.push(Instruction::Xor {
            a: 0,
            s: HIGH_MASKED,
            b: HIGH_ZERO,
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 0, s: 4, b: 0 });

        let second_guard = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, second_guard);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.guarded_values[0]));
        self.emit_branch_to(epilogue);

        self.bind_label(second_guard);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, plan.masks[1]));
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: Eabi::general_result().number,
            b: 0,
        });
        self.output.instructions.push(Instruction::Xor {
            a: Eabi::general_result().number,
            s: 0,
            b: HIGH_ZERO,
        });
        self.output.instructions.push(Instruction::Xor {
            a: 0,
            s: HIGH_MASKED,
            b: HIGH_ZERO,
        });
        self.output.instructions.push(Instruction::OrRecord {
            a: 0,
            s: Eabi::general_result().number,
            b: 0,
        });
        let default_result = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, default_result);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.guarded_values[1]));
        self.emit_branch_to(epilogue);

        self.bind_label(default_result);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.default_value));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 8,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }

    fn emit_scalarized_wide_mask_chain(&mut self, plan: &WideMaskChain, masks: [(u8, u8); 2]) {
        self.non_leaf = true;
        self.frame_size = 16;
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        // The GC 4.1 scalar proof retains five optimizer nodes before the
        // function's unwind-table symbols.
        self.output.anonymous_label_bump += 5;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, &plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.clone(),
        });
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 0,
            s: Eabi::general_result().number,
            begin: masks[0].0,
            end: masks[0].1,
        });
        self.record_relocation_with_addend(RelocationKind::EmbSda21, &plan.global, 4);
        self.output.instructions.push(Instruction::StoreWord {
            s: Eabi::general_result().number,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, &plan.global);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });

        let second_guard = self.fresh_label();
        let default_result = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, second_guard);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.guarded_values[0]));
        self.emit_branch_to(epilogue);

        self.bind_label(second_guard);
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: 0,
            s: Eabi::general_result().number,
            begin: masks[1].0,
            end: masks[1].1,
        });
        self.emit_branch_conditional_to(12, 2, default_result);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.guarded_values[1]));
        self.emit_branch_to(epilogue);

        self.bind_label(default_result);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.default_value));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}

fn recognize(
    function: &Function,
    globals: &std::collections::HashMap<String, Type>,
    call_return_types: &std::collections::HashMap<String, Type>,
) -> Option<WideMaskChain> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || function.locals.len() != 1
        || function.statements.len() != 1
        || function.guards.len() != 2
        || function.asm_body.is_some()
    {
        return None;
    }
    let local = &function.locals[0];
    if local.declared_type != Type::UnsignedLongLong
        || local.initializer.is_some()
        || local.is_volatile
        || local.is_static
        || local.array_length.is_some()
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned,
        value:
            Expression::Assign {
                target,
                value,
            },
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Variable(global) = target.as_ref() else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = value.as_ref()
    else {
        return None;
    };
    if assigned != &local.name
        || !arguments.is_empty()
        || globals.get(global) != Some(&Type::UnsignedLongLong)
        || call_return_types.get(callee) != Some(&Type::UnsignedInt)
    {
        return None;
    }
    let first = mask_guard(&function.guards[0], &local.name)?;
    let second = mask_guard(&function.guards[1], &local.name)?;
    let default_value = function
        .return_expression
        .as_ref()
        .and_then(constant_value)
        .and_then(|value| i16::try_from(value).ok())?;
    Some(WideMaskChain {
        global: global.clone(),
        callee: callee.clone(),
        masks: [first.0, second.0],
        guarded_values: [first.1, second.1],
        default_value,
    })
}

fn mask_guard(guard: &GuardedReturn, local: &str) -> Option<(i16, i16)> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = &guard.condition
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == local) {
        return None;
    }
    let mask = constant_value(right).and_then(|value| i16::try_from(value).ok())?;
    let value = constant_value(&guard.value).and_then(|value| i16::try_from(value).ok())?;
    (mask > 0).then_some((mask, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_a_positive_low_word_mask_guard() {
        let guard = GuardedReturn {
            condition: Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable("events".into())),
                right: Box::new(Expression::IntegerLiteral(0x80)),
            },
            value: Expression::IntegerLiteral(2),
        };
        assert_eq!(mask_guard(&guard, "events"), Some((0x80, 2)));
        assert_eq!(mask_guard(&guard, "other"), None);
    }
}
