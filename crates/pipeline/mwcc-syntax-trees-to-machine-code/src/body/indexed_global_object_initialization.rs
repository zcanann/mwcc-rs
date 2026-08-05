//! Counted initialization of indexed global objects around a scalar setter.

#[allow(unused_imports)]
use super::*;

struct Plan<'a> {
    global: &'a str,
    first: &'a str,
    acquire: &'a str,
    release: &'a str,
    count: i16,
    stride: i16,
    member_offset: i16,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn indexed_global<'a>(expression: &'a Expression, index: &str) -> Option<&'a str> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Index { base, index: found } = operand.as_ref() else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    var(found, index).then_some(global)
}

fn setter_offset(function: &Function) -> Option<i16> {
    let [base, value] = function.parameters.as_slice() else {
        return None;
    };
    let [Statement::Store {
        target: Expression::Member {
            base: member_base,
            offset,
            member_type,
            index_stride: None,
        },
        value: stored,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    (function.return_type == Type::Void
        && function.locals.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && matches!(base.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        && value.parameter_type == *member_type
        && var(member_base, &base.name)
        && var(stored, &value.name))
    .then(|| i16::try_from(*offset).ok())
    .flatten()
}

fn classify<'a>(generator: &'a Generator, function: &'a Function) -> Option<Plan<'a>> {
    if generator.behavior.frame_convention != FrameConvention::Predecrement
        || generator.behavior.global_addressing != GlobalAddressing::Absolute
        || !generator.behavior.repeatable_scalar_member_setter_inlining
        || function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [index] = function.locals.as_slice() else {
        return None;
    };
    if index.declared_type != Type::Int || index.initializer.is_some() {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer, Expression::Assign { target, value }
        if var(target, &index.name) && constant_value(value) == Some(0))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Less, left, right
        } if var(left, &index.name) && constant_value(right).is_some())
        || !matches!(step, Expression::Assign { target, value }
            if var(target, &index.name)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add, left, right
                } if var(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let Expression::Binary { right: count, .. } = condition else {
        unreachable!()
    };
    let count = i16::try_from(constant_value(count)?).ok()?;
    let [
        Statement::Expression(Expression::Call { name: first, arguments: first_args }),
        Statement::Expression(Expression::Call { name: acquire, arguments: acquire_args }),
        Statement::Expression(Expression::Call { name: setter, arguments: setter_args }),
        Statement::Expression(Expression::Call { name: release, arguments: release_args }),
    ] = body.as_slice()
    else {
        return None;
    };
    let [first_object] = first_args.as_slice() else { return None; };
    let [acquire_object] = acquire_args.as_slice() else { return None; };
    let [setter_object, setter_value] = setter_args.as_slice() else { return None; };
    let [release_object] = release_args.as_slice() else { return None; };
    let global = indexed_global(first_object, &index.name)?;
    if indexed_global(acquire_object, &index.name) != Some(global)
        || indexed_global(setter_object, &index.name) != Some(global)
        || indexed_global(release_object, &index.name) != Some(global)
        || constant_value(setter_value) != Some(0)
    {
        return None;
    }
    let stride = match generator.globals.get(global) {
        Some(Type::Struct { size, .. }) => i16::try_from(*size).ok()?,
        _ => return None,
    };
    let member_offset = generator
        .inline_bodies
        .definition_body(setter)
        .and_then(setter_offset)?;
    Some(Plan {
        global,
        first,
        acquire,
        release,
        count,
        stride,
        member_offset,
    })
}

impl Generator {
    pub(crate) fn try_indexed_global_object_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(self, function) else {
            return Ok(false);
        };
        let global = plan.global.to_owned();
        let first = plan.first.to_owned();
        let acquire = plan.acquire.to_owned();
        let release = plan.release.to_owned();
        let (count, stride, member_offset) = (plan.count, plan.stride, plan.member_offset);
        let body = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31, 30, 29];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::MoveFromLinkRegister { d: 0 },
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &global);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 36 },
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::load_immediate(31, 0),
            Instruction::StoreWord { s: 30, a: 1, offset: 24 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &global);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 29, a: 1, offset: 20 },
            Instruction::load_immediate(29, 0),
        ]);
        self.bind_label(body);
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, &first);
        self.output.instructions.push(Instruction::BranchAndLink { target: first });
        self.output.instructions.push(Instruction::move_register(3, 30));
        self.record_relocation(RelocationKind::Rel24, &acquire);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: acquire },
            Instruction::StoreWord { s: 31, a: 30, offset: member_offset },
            Instruction::move_register(3, 30),
        ]);
        self.record_relocation(RelocationKind::Rel24, &release);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: release },
            Instruction::AddImmediate { d: 29, a: 29, immediate: 1 },
            Instruction::AddImmediate { d: 30, a: 30, immediate: stride },
            Instruction::CompareWordImmediate { a: 29, immediate: count },
        ]);
        self.emit_branch_conditional_to(12, 0, body);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::LoadWord { d: 30, a: 1, offset: 24 },
            Instruction::LoadWord { d: 29, a: 1, offset: 20 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
