//! Rotated searches whose loop cursor advances through a nested owner pointer.
//!
//! Unlike an ordinary `p = p->next` list search, the loop condition loads an
//! intermediate owner (`p->owner`) and the chase continues from that owner
//! (`p = p->owner->next`). MWCC reuses one register for both identities.

#[allow(unused_imports)]
use super::*;

struct NestedPointerSearch {
    initializer_offset: i16,
    link_offset: i16,
    chase_offset: i16,
    found: Expression,
    missing: Expression,
    owner: String,
}

impl Generator {
    /// Lower `while (p && p->owner) { if (p->owner == root) return C;
    /// p = p->owner->next; } return D;` as one destructive rotated chase.
    pub(crate) fn try_nested_pointer_search_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = nested_pointer_search(function) else {
            return Ok(false);
        };
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let Some(owner) = self.lookup_general(&plan.owner) else {
            return Ok(false);
        };
        let cursor = owner + 1;
        if owner != Eabi::general_result().number {
            return Ok(false);
        }

        self.output.instructions.push(Instruction::LoadWord {
            d: cursor,
            a: owner,
            offset: plan.initializer_offset,
        });
        let entry = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });

        let body = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: cursor, b: owner });
        let skip_found = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 0,
            });
        self.evaluate_tail(&plan.found, function.return_type, owner)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        let chase = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[skip_found]
        {
            *target = chase;
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: cursor,
            a: cursor,
            offset: plan.chase_offset,
        });

        let test = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[entry] {
            *target = test;
        }
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: cursor,
                immediate: 0,
            });
        let missing = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: cursor,
            a: cursor,
            offset: plan.link_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: cursor,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: body,
            });

        let missing_target = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[missing]
        {
            *target = missing_target;
        }
        self.evaluate_tail(&plan.missing, function.return_type, owner)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump = 6;
        Ok(true)
    }
}

fn nested_pointer_search(function: &Function) -> Option<NestedPointerSearch> {
    let [owner] = function.parameters.as_slice() else {
        return None;
    };
    let [cursor] = function.locals.as_slice() else {
        return None;
    };
    let Expression::Member {
        base: initializer_base,
        offset: initializer_offset,
        ..
    } = cursor.initializer.as_ref()?
    else {
        return None;
    };
    if !matches!(initializer_base.as_ref(), Expression::Variable(name) if name == &owner.name) {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right: link,
        }),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(left.as_ref(), Expression::Variable(name) if name == &cursor.name) {
        return None;
    }
    let Expression::Member {
        base: link_base,
        offset: link_offset,
        ..
    } = link.as_ref()
    else {
        return None;
    };
    if !matches!(link_base.as_ref(), Expression::Variable(name) if name == &cursor.name) {
        return None;
    }
    let [
        Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: compared_link,
                    right: compared_owner,
                },
            then_body,
            else_body,
        },
        Statement::Assign { name, value: chase },
    ] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || name != &cursor.name
        || !matches!(compared_owner.as_ref(), Expression::Variable(name) if name == &owner.name)
    {
        return None;
    }
    let Expression::Member {
        base: compared_base,
        offset: compared_offset,
        ..
    } = compared_link.as_ref()
    else {
        return None;
    };
    if compared_offset != link_offset
        || !matches!(compared_base.as_ref(), Expression::Variable(name) if name == &cursor.name)
    {
        return None;
    }
    let Expression::Member {
        base: chase_base,
        offset: chase_offset,
        ..
    } = chase
    else {
        return None;
    };
    let Expression::Member {
        base: nested_base,
        offset: nested_offset,
        ..
    } = chase_base.as_ref()
    else {
        return None;
    };
    if nested_offset != link_offset
        || !matches!(nested_base.as_ref(), Expression::Variable(name) if name == &cursor.name)
    {
        return None;
    }
    let [Statement::Return(Some(found))] = then_body.as_slice() else {
        return None;
    };
    let missing = function.return_expression.as_ref()?;
    constant_value(found)?;
    constant_value(missing)?;

    Some(NestedPointerSearch {
        initializer_offset: i16::try_from(*initializer_offset).ok()?,
        link_offset: i16::try_from(*link_offset).ok()?,
        chase_offset: i16::try_from(*chase_offset).ok()?,
        found: found.clone(),
        missing: missing.clone(),
        owner: owner.name.clone(),
    })
}
