//! Move up to a requested number of objects from a global pool into a manager.
//!
//! The linkage-first allocator gives every value spanning the two list calls a
//! stable home: both parameters, the count, the returned object, the two list
//! addresses, and the writable-section anchor. Besides matching the dense
//! `stmw` frame, preserving the manager list address is semantically important:
//! reconstructing it from an address-taken parameter slot can otherwise select
//! unrelated scratch space in the frame.

#[allow(unused_imports)]
use super::*;

struct AllocateFromGlobalPool<'a> {
    global: &'a str,
    global_count_offset: i16,
    global_list_offset: i16,
    manager_count_offset: i16,
    manager_list_offset: i16,
    object_manager_offset: i16,
    take: &'a str,
    append: &'a str,
    initialize: &'a str,
}

impl Generator {
    pub(crate) fn try_allocate_from_global_pool(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.use_lmw_stmw
            || !self.full_bss_globals.contains(plan.global)
        {
            return Ok(false);
        }
        let planned_anchor = self
            .data_section_anchor
            .as_ref()
            .filter(|anchor| anchor.symbols.contains(plan.global));
        let anchor_symbol = planned_anchor
            .map(|anchor| anchor.anchor_symbol.clone())
            .unwrap_or_else(|| "...bss.0".to_owned());

        const OBJECT: u8 = 25;
        const COUNT: u8 = 26;
        const MANAGER: u8 = 27;
        const LIMIT: u8 = 28;
        const MANAGER_LIST: u8 = 29;
        const GLOBAL_LIST: u8 = 30;
        const ANCHOR: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 56;
        self.callee_saved = vec![
            ANCHOR,
            GLOBAL_LIST,
            MANAGER_LIST,
            LIMIT,
            MANAGER,
            COUNT,
            OBJECT,
        ];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        if let Some(anchor) = self.data_section_anchor.as_mut() {
            anchor.register = Some(ANCHOR);
        } else {
            self.data_section_anchor = Some(DataSectionAnchorPlan {
                symbols: std::collections::HashSet::from([plan.global.to_owned()]),
                anchor_symbol: anchor_symbol.clone(),
                register: Some(ANCHOR),
            });
        }

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -56 },
            Instruction::StoreMultipleWord { s: OBJECT, a: 1, offset: 28 },
            Instruction::load_immediate(COUNT, 0),
            Instruction::StoreWord { s: 3, a: 1, offset: 8 },
        ]);
        self.emit_address_high(3, &anchor_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, &anchor_symbol);
        self.output.instructions.push(Instruction::AddImmediate {
            d: ANCHOR,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.extend([
            Instruction::StoreWord { s: 4, a: 1, offset: 12 },
        ]);
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: GLOBAL_LIST,
                a: ANCHOR,
                immediate: plan.global_list_offset,
            },
            Instruction::LoadWord { d: MANAGER, a: 1, offset: 8 },
            Instruction::LoadWord { d: LIMIT, a: 1, offset: 12 },
            Instruction::AddImmediate {
                d: MANAGER_LIST,
                a: MANAGER,
                immediate: plan.manager_list_offset,
            },
            Instruction::Branch { target: 25 },
            Instruction::move_register(3, GLOBAL_LIST),
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.take);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.take.to_owned(),
        });
        self.output.instructions.extend([
            Instruction::OrRecord { a: OBJECT, s: 3, b: 3 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 27,
            },
            Instruction::AddImmediate {
                d: 3,
                a: MANAGER_LIST,
                immediate: 0,
            },
            Instruction::AddImmediate { d: 4, a: OBJECT, immediate: 0 },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.append);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.append.to_owned(),
        });
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: MANAGER,
                a: OBJECT,
                offset: plan.object_manager_offset,
            },
            Instruction::move_register(3, OBJECT),
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.initialize);
        self.output.instructions.extend([
            Instruction::BranchAndLink {
                target: plan.initialize.to_owned(),
            },
            Instruction::AddImmediate { d: COUNT, a: COUNT, immediate: 1 },
            Instruction::CompareLogicalWord { a: COUNT, b: LIMIT },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 14,
            },
            Instruction::LoadWord {
                d: 0,
                a: MANAGER,
                offset: plan.manager_count_offset,
            },
            Instruction::AddImmediate { d: 3, a: COUNT, immediate: 0 },
            Instruction::Add { d: 0, a: 0, b: COUNT },
            Instruction::StoreWord {
                s: 0,
                a: MANAGER,
                offset: plan.manager_count_offset,
            },
        ]);
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: ANCHOR,
            offset: plan.global_count_offset,
        });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: 0,
            a: COUNT,
            b: 0,
        });
        self.record_data_section_symbol_displacement(plan.global);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 0,
                a: ANCHOR,
                offset: plan.global_count_offset,
            },
            Instruction::LoadWord { d: 0, a: 1, offset: 60 },
            Instruction::LoadMultipleWord { d: OBJECT, a: 1, offset: 28 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 56 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<AllocateFromGlobalPool<'_>> {
    let [manager_parameter, limit_parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [manager_alias, limit_alias, count_local, object_local] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || !matches!(manager_parameter.parameter_type, Type::StructPointer { .. })
        || limit_parameter.parameter_type != Type::UnsignedInt
        || !address_aliases(manager_alias, &manager_parameter.name)
        || !address_aliases(limit_alias, &limit_parameter.name)
        || count_local.declared_type != Type::Int
        || constant_value(count_local.initializer.as_ref()?) != Some(0)
        || !matches!(object_local.declared_type, Type::StructPointer { .. })
        || object_local.initializer.is_some()
        || !function.guards.is_empty()
        || variable(function.return_expression.as_ref()?) != Some(count_local.name.as_str())
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }, manager_update, global_update] = function.statements.as_slice()
    else {
        return None;
    };
    if !binary_variables(
        condition,
        BinaryOperator::Less,
        &count_local.name,
        &limit_parameter.name,
    ) {
        return None;
    }
    let [Statement::Assign { name: object_name, value: take_call }, null_break, append_statement, publish_manager, initialize_statement, increment] =
        body.as_slice()
    else {
        return None;
    };
    if object_name != &object_local.name {
        return None;
    }
    let Expression::Call { name: take, arguments: take_arguments } = take_call else {
        return None;
    };
    let [Expression::AddressOf { operand: global_list }] = take_arguments.as_slice() else {
        return None;
    };
    let (global_base, global_list_offset) = member(global_list)?;
    let global = variable(global_base)?;

    let Statement::If { condition: null_condition, then_body, else_body } = null_break else {
        return None;
    };
    if !binary_variable_constant(
        null_condition,
        BinaryOperator::Equal,
        &object_local.name,
        0,
    ) || !matches!(then_body.as_slice(), [Statement::Break])
        || !else_body.is_empty()
    {
        return None;
    }

    let (append, append_arguments) = direct_call_statement(append_statement)?;
    let [Expression::AddressOf { operand: manager_list }, appended_object] = append_arguments else {
        return None;
    };
    let (manager_base, manager_list_offset) = member(manager_list)?;
    if variable(manager_base) != Some(manager_parameter.name.as_str())
        || variable(appended_object) != Some(object_local.name.as_str())
    {
        return None;
    }

    let (publish_target, publish_value) = store(publish_manager)?;
    let (published_object, object_manager_offset) = member(publish_target)?;
    if variable(published_object) != Some(object_local.name.as_str())
        || variable(publish_value) != Some(manager_parameter.name.as_str())
    {
        return None;
    }
    let (initialize, initialize_arguments) = direct_call_statement(initialize_statement)?;
    if !single_variable_argument(initialize_arguments, &object_local.name)
        || !increments_variable(increment, &count_local.name)
    {
        return None;
    }

    let (manager_count_target, manager_count_value) = store(manager_update)?;
    let (manager_count_base, manager_count_offset) = member(manager_count_target)?;
    if variable(manager_count_base) != Some(manager_parameter.name.as_str())
        || !updated_by_variable(
            manager_count_target,
            manager_count_value,
            BinaryOperator::Add,
            &count_local.name,
        )
    {
        return None;
    }
    let (global_count_target, global_count_value) = store(global_update)?;
    let (global_count_base, global_count_offset) = member(global_count_target)?;
    if variable(global_count_base) != Some(global)
        || !updated_by_variable(
            global_count_target,
            global_count_value,
            BinaryOperator::Subtract,
            &count_local.name,
        )
    {
        return None;
    }

    Some(AllocateFromGlobalPool {
        global,
        global_count_offset: i16::try_from(global_count_offset).ok()?,
        global_list_offset: i16::try_from(global_list_offset).ok()?,
        manager_count_offset: i16::try_from(manager_count_offset).ok()?,
        manager_list_offset: i16::try_from(manager_list_offset).ok()?,
        object_manager_offset: i16::try_from(object_manager_offset).ok()?,
        take,
        append,
        initialize,
    })
}

fn address_aliases(local: &LocalDeclaration, parameter: &str) -> bool {
    matches!(local.initializer.as_ref(), Some(Expression::AddressOf { operand })
        if variable(operand) == Some(parameter))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member { base, offset, index_stride: None, .. } = expression else {
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

fn direct_call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn single_variable_argument(arguments: &[Expression], expected: &str) -> bool {
    matches!(arguments, [argument] if variable(argument) == Some(expected))
}

fn binary_variables(
    expression: &Expression,
    operator: BinaryOperator,
    left_name: &str,
    right_name: &str,
) -> bool {
    matches!(expression, Expression::Binary { operator: candidate, left, right }
        if *candidate == operator
            && variable(left) == Some(left_name)
            && variable(right) == Some(right_name))
}

fn binary_variable_constant(
    expression: &Expression,
    operator: BinaryOperator,
    variable_name: &str,
    constant: i64,
) -> bool {
    matches!(expression, Expression::Binary { operator: candidate, left, right }
        if *candidate == operator
            && variable(left) == Some(variable_name)
            && constant_value(right) == Some(constant))
}

fn increments_variable(statement: &Statement, name: &str) -> bool {
    matches!(statement, Statement::Assign { name: assigned, value }
        if assigned == name
            && matches!(value, Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if variable(left) == Some(name) && constant_value(right) == Some(1)))
}

fn updated_by_variable(
    target: &Expression,
    value: &Expression,
    operator: BinaryOperator,
    variable_name: &str,
) -> bool {
    let Expression::IndexedUpdateValue { value } = value else {
        return false;
    };
    matches!(value.as_ref(), Expression::Binary {
        operator: candidate,
        left,
        right,
    } if *candidate == operator
        && structurally_equal(target, left)
        && variable(right) == Some(variable_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_body_without_the_complete_pool_transaction() {
        let function = Function {
            return_type: Type::Int,
            name: "not_a_pool_move".into(),
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
