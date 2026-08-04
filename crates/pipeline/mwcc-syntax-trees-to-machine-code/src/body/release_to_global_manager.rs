//! Release an object into a global manager after publishing it to a list.
//!
//! The object and the writable-section anchor both span the leading list call.
//! Treating the global address as an ordinary call argument can clobber the
//! incoming object before it reaches its saved home. This owner recognizes the
//! complete transaction and gives both persistent values explicit lifetimes.

#[allow(unused_imports)]
use super::*;

struct ReleaseToGlobalManager<'a> {
    global: &'a str,
    callee: &'a str,
    list_offset: i16,
    manager_offset: i16,
    manager_count_offset: i16,
    global_count_offset: i16,
}

impl Generator {
    pub(crate) fn try_release_to_global_manager(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.use_lmw_stmw
            || !self.full_bss_globals.contains(plan.global)
        {
            return Ok(false);
        }
        let Some(anchor) = self
            .data_section_anchor
            .as_ref()
            .filter(|anchor| anchor.symbols.contains(plan.global))
        else {
            return Ok(false);
        };
        let anchor_symbol = anchor.anchor_symbol.clone();

        const OBJECT: u8 = 30;
        const MANAGER: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![MANAGER, OBJECT];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        if let Some(anchor) = self.data_section_anchor.as_mut() {
            anchor.register = Some(MANAGER);
        }

        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(4, &anchor_symbol);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreMultipleWord { s: OBJECT, a: 1, offset: 16 },
            Instruction::AddImmediate {
                d: OBJECT,
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &anchor_symbol);
        self.output.instructions.push(Instruction::AddImmediate {
            d: MANAGER,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: OBJECT,
            immediate: 0,
        });
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: MANAGER,
            immediate: plan.list_offset,
        });
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_owned(),
        });

        self.output.instructions.extend([
            Instruction::LoadWord { d: 5, a: OBJECT, offset: plan.manager_offset },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord { d: 4, a: 5, offset: plan.manager_count_offset },
            Instruction::AddImmediate { d: 0, a: 4, immediate: -1 },
            Instruction::StoreWord { s: 0, a: 5, offset: plan.manager_count_offset },
        ]);
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: MANAGER,
            offset: plan.global_count_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 1,
        });
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: MANAGER, offset: plan.global_count_offset },
            Instruction::StoreWord { s: MANAGER, a: OBJECT, offset: plan.manager_offset },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadMultipleWord { d: OBJECT, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<ReleaseToGlobalManager<'_>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !matches!(parameter.parameter_type, Type::StructPointer { .. })
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name: callee, arguments }), decrement, increment, publish] =
        function.statements.as_slice()
    else {
        return None;
    };
    let [Expression::AddressOf { operand: list }, object_argument] = arguments.as_slice() else {
        return None;
    };
    if variable(object_argument) != Some(parameter.name.as_str()) {
        return None;
    }
    let (list_base, list_offset) = member(list)?;
    let global = variable(list_base)?;

    let (decrement_target, decrement_value) = store(decrement)?;
    let (manager_base, manager_count_offset) = member(decrement_target)?;
    let (object_base, manager_offset) = member(manager_base)?;
    if variable(object_base) != Some(parameter.name.as_str())
        || !updated_by_one(decrement_target, decrement_value, BinaryOperator::Subtract)
    {
        return None;
    }

    let (increment_target, increment_value) = store(increment)?;
    let (increment_base, global_count_offset) = member(increment_target)?;
    if variable(increment_base) != Some(global)
        || !updated_by_one(increment_target, increment_value, BinaryOperator::Add)
    {
        return None;
    }

    let (publish_target, publish_value) = store(publish)?;
    let (publish_base, publish_offset) = member(publish_target)?;
    let Expression::AddressOf { operand: published_manager } = publish_value else {
        return None;
    };
    if variable(publish_base) != Some(parameter.name.as_str())
        || publish_offset != manager_offset
        || variable(published_manager) != Some(global)
    {
        return None;
    }

    Some(ReleaseToGlobalManager {
        global,
        callee,
        list_offset: i16::try_from(list_offset).ok()?,
        manager_offset: i16::try_from(manager_offset).ok()?,
        manager_count_offset: i16::try_from(manager_count_offset).ok()?,
        global_count_offset: i16::try_from(global_count_offset).ok()?,
    })
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    Some((base, *offset))
}

fn store(statement: &Statement) -> Option<(&Expression, &Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    Some((target, value))
}

fn updated_by_one(target: &Expression, value: &Expression, operator: BinaryOperator) -> bool {
    let Expression::IndexedUpdateValue { value } = value else {
        return false;
    };
    matches!(value.as_ref(), Expression::Binary {
        operator: candidate,
        left,
        right,
    } if *candidate == operator
        && structurally_equal(target, left)
        && constant_value(right) == Some(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unrelated_call_followed_by_stores() {
        let function = Function {
            return_type: Type::Int,
            name: "not_a_release".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![],
            statements: vec![],
            guards: vec![],
            return_expression: Some(Expression::IntegerLiteral(0)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(classify(&function).is_none());
    }
}
