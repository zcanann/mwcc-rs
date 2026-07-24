//! Two-arm friction selection returning which velocity limit was exceeded.

#[allow(unused_imports)]
use super::*;

enum Limit {
    Parameter,
    Member { base_offset: i16, member_offset: i16 },
}

enum Source<'a> {
    Global { name: &'a str, offset: i16 },
    Member { offset: i16 },
}

struct Shape<'a> {
    pointer: &'a str,
    velocity_offset: i16,
    output_offset: i16,
    limit: Limit,
    high: Source<'a>,
    low: Source<'a>,
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(value) if value == name)
}

fn member(expression: &Expression, base: &str) -> Option<i16> {
    let Expression::Member { base: expression_base, offset, member_type: Type::Float, index_stride: None } = expression else {
        return None;
    };
    variable(expression_base, base).then(|| i16::try_from(*offset).ok()).flatten()
}

fn absolute_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Conditional { condition, when_true, when_false, .. }
        if matches!(condition.as_ref(), Expression::Binary { operator: BinaryOperator::Less, left, right }
            if variable(left, name) && is_zero_literal(right))
        && matches!(when_true.as_ref(), Expression::Unary { operator: UnaryOperator::Negate, operand }
            if variable(operand, name))
        && variable(when_false, name))
}

fn negates_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, operand }
        if variable(operand, name))
}

fn source<'a>(expression: &'a Expression, pointer: &str) -> Option<Source<'a>> {
    let Expression::Member { base, offset, member_type: Type::Float, index_stride: None } = expression else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else { return None; };
    let offset = i16::try_from(*offset).ok()?;
    Some(if base == pointer { Source::Member { offset } } else { Source::Global { name: base, offset } })
}

fn arm<'a>(statements: &'a [Statement], pointer: &str, velocity: &str, result: i64) -> Option<(Source<'a>, i16)> {
    let [Statement::Assign { name: selected, value: initial }, Statement::If { condition, then_body, else_body }, Statement::Store { target, value }, Statement::Return(Some(Expression::IntegerLiteral(returned)))] = statements else {
        return None;
    };
    if *returned != result || !variable(value, selected) { return None; }
    let output = member(target, pointer)?;
    let Expression::Binary { operator: BinaryOperator::GreaterEqual, left, right } = condition else { return None; };
    if !absolute_variable(left, selected) || !absolute_variable(right, velocity) { return None; }
    if !matches!(then_body.as_slice(), [Statement::Assign { name, value }]
        if name == selected && negates_variable(value, velocity)) { return None; }
    let [Statement::If { condition, then_body, else_body: sign_else }] = else_body.as_slice() else { return None; };
    if !sign_else.is_empty()
        || !matches!(condition, Expression::Binary { operator: BinaryOperator::Greater, left, right }
            if variable(left, velocity) && is_zero_literal(right))
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value: Expression::Unary { operator: UnaryOperator::Negate, operand } }]
            if name == selected && same_operand(operand, initial))
    { return None; }
    Some((source(initial, pointer)?, output))
}

fn classify(function: &Function) -> Option<Shape<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() || function.return_expression.is_some() || function_makes_call(function) { return None; }
    let pointer = function.parameters.first()?;
    if !matches!(pointer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. }) { return None; }
    let velocity = function.locals.iter().find(|local| local.declared_type == Type::Float && local.initializer.as_ref().and_then(|value| member(value, &pointer.name)).is_some())?;
    let velocity_offset = member(velocity.initializer.as_ref()?, &pointer.name)?;
    let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else { return None; };
    let Expression::Binary { operator: BinaryOperator::Greater, left, right } = condition else { return None; };
    if !absolute_variable(left, &velocity.name) { return None; }
    let limit = if function.parameters.get(1).is_some_and(|parameter| parameter.parameter_type == Type::Float && variable(right, &parameter.name)) {
        Limit::Parameter
    } else {
        let Expression::Member { base, offset, member_type: Type::Float, index_stride: None } = right.as_ref() else { return None; };
        let Expression::Variable(alias) = base.as_ref() else { return None; };
        let alias = function.locals.iter().find(|local| local.name == *alias)?;
        let Expression::AddressOf { operand } = alias.initializer.as_ref()? else { return None; };
        let Expression::Member { base, offset: base_offset, member_type: Type::Struct { .. }, index_stride: None } = operand.as_ref() else { return None; };
        if !variable(base, &pointer.name) { return None; }
        Limit::Member { base_offset: i16::try_from(*base_offset).ok()?, member_offset: i16::try_from(*offset).ok()? }
    };
    let (high, high_output) = arm(then_body, &pointer.name, &velocity.name, 1)?;
    let (low, low_output) = arm(else_body, &pointer.name, &velocity.name, 0)?;
    if high_output != low_output || !matches!(high, Source::Global { .. }) || !matches!(low, Source::Member { .. }) { return None; }
    Some(Shape { pointer: &pointer.name, velocity_offset, output_offset: high_output, limit, high, low })
}

impl Generator {
    pub(crate) fn try_conditional_friction_select(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else { return Ok(false); };
        let pointer = self.general_register_of(shape.pointer)?;
        if pointer != 3 { return Ok(false); }
        self.output.pre_scheduled = true;
        self.output.has_float_branch = true;
        self.output.anonymous_label_bump += 24;
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 3, a: pointer, offset: shape.velocity_offset });
        let absolute = match shape.limit {
            Limit::Member { base_offset, .. } => {
                self.output.instructions.push(Instruction::AddImmediate { d: 4, a: pointer, immediate: base_offset });
                1
            }
            Limit::Parameter => 0,
        };
        self.emit_absolute_velocity(absolute);
        match shape.limit {
            Limit::Parameter => self.output.instructions.push(Instruction::FloatCompareOrdered { a: 0, b: 1 }),
            Limit::Member { member_offset, .. } => {
                self.output.instructions.push(Instruction::LoadFloatSingle { d: 0, a: 4, offset: member_offset });
                self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
            }
        }
        let low = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, low);
        self.emit_conditional_friction_arm(pointer, shape.output_offset, shape.high, 1)?;
        self.bind_label(low);
        self.emit_conditional_friction_arm(pointer, shape.output_offset, shape.low, 0)?;
        Ok(true)
    }

    fn emit_absolute_velocity(&mut self, destination: u8) {
        self.load_float_constant(0, 0.0);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        let nonnegative = self.fresh_label();
        let done = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, nonnegative);
        self.output.instructions.push(Instruction::FloatNegate { d: destination, b: 3 });
        self.emit_branch_to(done);
        self.bind_label(nonnegative);
        self.output.instructions.push(Instruction::FloatMove { d: destination, b: 3 });
        self.bind_label(done);
    }

    fn emit_conditional_friction_arm(&mut self, pointer: u8, output_offset: i16, source: Source<'_>, result: i16) -> Compilation<()> {
        self.load_float_constant(0, 0.0);
        match source {
            Source::Global { name, offset } => self.emit_global_load_value(name, 4).map(|_| self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: 4, offset }))?,
            Source::Member { offset } => self.output.instructions.push(Instruction::LoadFloatSingle { d: 2, a: pointer, offset }),
        }
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        // MWCC fills the compare/branch gap with the global candidate load.
        if matches!(source, Source::Global { .. }) {
            let length = self.output.instructions.len();
            self.output.instructions.swap(length - 1, length - 2);
        }
        let velocity_nonnegative = self.fresh_label(); let velocity_absolute = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, velocity_nonnegative);
        self.output.instructions.push(Instruction::FloatNegate { d: 1, b: 3 }); self.emit_branch_to(velocity_absolute);
        self.bind_label(velocity_nonnegative); self.output.instructions.push(Instruction::FloatMove { d: 1, b: 3 }); self.bind_label(velocity_absolute);
        self.load_float_constant(0, 0.0);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        let source_nonnegative = self.fresh_label(); let source_absolute = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, source_nonnegative);
        self.output.instructions.push(Instruction::FloatNegate { d: 0, b: 2 }); self.emit_branch_to(source_absolute);
        self.bind_label(source_nonnegative); self.output.instructions.push(Instruction::FloatMove { d: 0, b: 2 }); self.bind_label(source_absolute);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 0, b: 1 });
        self.output.instructions.push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        let adjust = self.fresh_label(); let done = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, adjust);
        self.output.instructions.push(Instruction::FloatNegate { d: 2, b: 3 }); self.emit_branch_to(done);
        self.bind_label(adjust); self.load_float_constant(0, 0.0);
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, done);
        self.output.instructions.push(Instruction::FloatNegate { d: 2, b: 2 }); self.bind_label(done);
        self.output.instructions.push(Instruction::StoreFloatSingle { s: 2, a: pointer, offset: output_offset });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 0, immediate: result });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }
}
