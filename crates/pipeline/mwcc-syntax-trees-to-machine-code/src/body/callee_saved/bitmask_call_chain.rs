//! A saved dirty-bit mask driving a chain of conditional SDK calls.

#[allow(unused_imports)]
use super::*;

pub(super) struct BitCall<'a> {
    pub(super) mask: u32,
    pub(super) callee: &'a str,
}

/// Convert a nonzero contiguous bit mask to the PowerPC `rlwinm.` mask bounds.
pub(super) fn rotate_mask_bounds(mask: u32) -> Option<(u8, u8)> {
    if mask == 0 {
        return None;
    }
    let low = mask.trailing_zeros();
    let high = 31 - mask.leading_zeros();
    let width = high - low + 1;
    let normalized = mask >> low;
    let expected = if width == 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    };
    (normalized == expected).then_some(((31 - high) as u8, (31 - low) as u8))
}

pub(super) fn recognize_bit_calls<'a>(
    statements: &'a [Statement],
    mask_name: &str,
) -> Option<Vec<BitCall<'a>>> {
    if statements.is_empty() {
        return None;
    }
    let mut calls = Vec::with_capacity(statements.len());
    for statement in statements {
        let Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left,
                    right,
                },
            then_body,
            else_body,
        } = statement
        else {
            return None;
        };
        let [Statement::Expression(Expression::Call { name, arguments })] = then_body.as_slice()
        else {
            return None;
        };
        let mask = constant_value(right).and_then(|value| u32::try_from(value).ok())?;
        if !matches!(left.as_ref(), Expression::Variable(name) if name == mask_name)
            || !else_body.is_empty()
            || !arguments.is_empty()
            || rotate_mask_bounds(mask).is_none()
        {
            return None;
        }
        calls.push(BitCall { mask, callee: name });
    }
    Some(calls)
}

impl Generator {
    /// Lower the common Dolphin SDK dirty-state dispatcher:
    ///
    /// ```text
    /// flags = state->dirty;
    /// if (flags & MASK_A) call_a();
    /// if (flags & MASK_B) call_b();
    /// ...
    /// state->dirty = 0;
    /// ```
    ///
    /// The memory-loaded mask lives in r31 across every call. Each contiguous mask becomes one
    /// recorded rotate/mask test followed by a branch around its argument-free call. The measured
    /// owner is the linkage-first SDK generation; later frame/scheduling conventions remain in the
    /// failure pool until characterized.
    pub(crate) fn try_callee_saved_bitmask_call_chain(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.declared_type != Type::UnsignedInt
            || local.array_length.is_some()
            || local.is_static
        {
            return Ok(false);
        }
        let Some(Expression::Member {
            base: initializer_base,
            offset: initializer_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }) = local.initializer.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(initializer_global) = initializer_base.as_ref() else {
            return Ok(false);
        };
        let Some(&initializer_base_type) = self.globals.get(initializer_global.as_str()) else {
            return Ok(false);
        };
        let Ok(initializer_displacement) = i16::try_from(*initializer_offset) else {
            return Ok(false);
        };

        let Some((trailing, conditional_statements)) = function.statements.split_last() else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: trailing_base,
                    offset: trailing_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value: Expression::IntegerLiteral(0),
        } = trailing
        else {
            return Ok(false);
        };
        if *trailing_offset != *initializer_offset
            || !matches!(trailing_base.as_ref(), Expression::Variable(name) if name == initializer_global)
            || conditional_statements.is_empty()
        {
            return Ok(false);
        }

        let Some(calls) = recognize_bit_calls(conditional_statements, &local.name) else {
            return Ok(false);
        };

        const SAVED_MASK: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![SAVED_MASK];
        self.output.pre_scheduled = true;
        self.output.anonymous_label_bump = 1 + 2 * calls.len() as u32;

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
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
            s: SAVED_MASK,
            a: 1,
            offset: 12,
        });
        // Keep r3 as the member base while r31 receives the saved word. Asking the generic member
        // evaluator for destination r31 would legally reuse r31 as its own base, but this SDK
        // generation deliberately leaves the global pointer in the result/argument register.
        self.evaluate(initializer_base, initializer_base_type, 3)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: SAVED_MASK,
            a: 3,
            offset: initializer_displacement,
        });

        self.emit_saved_bit_calls(&calls, SAVED_MASK);

        self.emit_store(
            match trailing {
                Statement::Store { target, .. } => target,
                _ => unreachable!(),
            },
            &Expression::IntegerLiteral(0),
        )?;
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: SAVED_MASK,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    pub(super) fn emit_saved_bit_calls(&mut self, calls: &[BitCall<'_>], saved_mask: u8) {
        for call in calls {
            let (begin, end) = rotate_mask_bounds(call.mask).expect("gated contiguous mask");
            if call.mask == 1 {
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediateRecord {
                        a: 0,
                        s: saved_mask,
                        clear: 31,
                    });
            } else {
                self.output
                    .instructions
                    .push(Instruction::RotateAndMaskRecord {
                        a: 0,
                        s: saved_mask,
                        shift: 0,
                        begin,
                        end,
                    });
            }
            let skip = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 0,
                });
            self.record_relocation(RelocationKind::Rel24, call.callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: call.callee.to_string(),
            });
            let after_call = self.output.instructions.len();
            let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[skip]
            else {
                unreachable!()
            };
            *target = after_call;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::rotate_mask_bounds;

    #[test]
    fn contiguous_masks_map_to_powerpc_bit_numbers() {
        assert_eq!(rotate_mask_bounds(1), Some((31, 31)));
        assert_eq!(rotate_mask_bounds(2), Some((30, 30)));
        assert_eq!(rotate_mask_bounds(24), Some((27, 28)));
        assert_eq!(rotate_mask_bounds(0), None);
        assert_eq!(rotate_mask_bounds(5), None);
    }
}
